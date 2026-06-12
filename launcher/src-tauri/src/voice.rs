//! Synthesizes Win+H to invoke Windows 11 built-in voice typing into the
//! currently-focused edit field (the palette input). Cloud-based (Azure),
//! zh-TW supported; requires the OS "online speech recognition" setting.

#[cfg(windows)]
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS,
    KEYEVENTF_KEYUP, VIRTUAL_KEY, VK_LWIN,
};

/// 'H' virtual-key code (no named constant in the windows crate for letters).
#[cfg(windows)]
const VK_H: VIRTUAL_KEY = VIRTUAL_KEY(0x48);

/// Keys that must be physically released before injecting Win+H — every key
/// that can plausibly be part of the user's trigger hotkey (default Alt+H):
/// VK_MENU(Alt), VK_LWIN, VK_RWIN, VK_CONTROL, VK_SHIFT, and 'H' itself.
#[cfg(windows)]
const HOTKEY_VKS: [i32; 6] = [0x12, 0x5B, 0x5C, 0x11, 0x10, 0x48];

/// Wait (≤1s, 10ms polls) until Alt/Win/Ctrl/Shift and 'H' are physically
/// released — injecting Win+H while the user still holds the hotkey's Alt
/// yields Win+Alt+H (a different OS chord) instead of voice typing.
/// `probe(vk)` returns whether that virtual key is currently held; injected
/// as a parameter so the wait loop is unit-testable without real keyboards.
#[cfg(windows)]
fn wait_for_modifier_release(probe: impl Fn(i32) -> bool) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(1);
    while HOTKEY_VKS.iter().any(|&vk| probe(vk)) {
        if std::time::Instant::now() >= deadline {
            return; // 1s cap: give up and inject anyway (best effort)
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

/// Production probe: GetAsyncKeyState high bit = key is physically down now.
#[cfg(windows)]
fn key_held(vk: i32) -> bool {
    // SAFETY: GetAsyncKeyState has no preconditions; any i32 vk is accepted.
    (unsafe { GetAsyncKeyState(vk) } as u16) & 0x8000 != 0
}

#[cfg(windows)]
fn key_input(vk: VIRTUAL_KEY, up: bool) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: 0,
                dwFlags: if up { KEYEVENTF_KEYUP } else { KEYBD_EVENT_FLAGS(0) },
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

/// The Win+H chord as (virtual key, is_key_up) pairs, in send order.
/// Kept as data so the sequence is unit-testable without touching SendInput.
#[cfg(windows)]
const WIN_H_SEQUENCE: [(VIRTUAL_KEY, bool); 4] =
    [(VK_LWIN, false), (VK_H, false), (VK_H, true), (VK_LWIN, true)];

/// Pure builder: LWIN down, H down, H up, LWIN up.
#[cfg(windows)]
pub fn build_win_h_inputs() -> [INPUT; 4] {
    WIN_H_SEQUENCE.map(|(vk, up)| key_input(vk, up))
}

/// Sends the Win+H chord to the foreground window (i.e. opens voice typing),
/// after waiting for the physical hotkey keys to be released (otherwise the
/// still-held Alt turns the injected chord into Win+Alt+H).
/// Err when SendInput injected nothing (e.g. blocked by a higher-integrity
/// foreground process) so the caller can surface it.
#[cfg(windows)]
pub fn trigger_voice_typing() -> Result<(), String> {
    wait_for_modifier_release(key_held);
    let inputs = build_win_h_inputs();
    // SAFETY: `inputs` is a valid, properly-initialized INPUT array and the
    // size argument matches the element type, per the SendInput contract.
    let sent = unsafe { SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) };
    if sent == 0 {
        return Err("無法送出語音輸入快捷鍵(Win+H)".into());
    }
    Ok(())
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn win_h_sequence_is_lwin_down_h_down_h_up_lwin_up() {
        assert_eq!(
            WIN_H_SEQUENCE,
            [(VK_LWIN, false), (VK_H, false), (VK_H, true), (VK_LWIN, true)]
        );
    }

    #[test]
    fn build_win_h_inputs_produces_4_keyboard_inputs_with_keyup_flags() {
        let inputs = build_win_h_inputs();
        assert_eq!(inputs.len(), 4);
        let expect = [
            (VK_LWIN, KEYBD_EVENT_FLAGS(0)),
            (VK_H, KEYBD_EVENT_FLAGS(0)),
            (VK_H, KEYEVENTF_KEYUP),
            (VK_LWIN, KEYEVENTF_KEYUP),
        ];
        for (i, (input, (vk, flags))) in inputs.iter().zip(expect).enumerate() {
            assert_eq!(input.r#type, INPUT_KEYBOARD, "entry {i} type");
            // SAFETY: every entry is built as INPUT_KEYBOARD, so the `ki`
            // union member is the initialized one.
            let ki = unsafe { input.Anonymous.ki };
            assert_eq!(ki.wVk, vk, "entry {i} virtual key");
            assert_eq!(ki.dwFlags, flags, "entry {i} flags");
            assert_eq!(ki.wScan, 0, "entry {i} scan code");
        }
    }

    #[test]
    fn wait_for_modifier_release_returns_immediately_when_nothing_held() {
        let start = std::time::Instant::now();
        wait_for_modifier_release(|_| false);
        assert!(
            start.elapsed() < std::time::Duration::from_millis(200),
            "全部已放開 → 不得進入輪詢等待"
        );
    }

    #[test]
    fn wait_for_modifier_release_probes_alt_win_ctrl_shift_and_h() {
        let seen = std::cell::RefCell::new(std::collections::BTreeSet::new());
        wait_for_modifier_release(|vk| {
            seen.borrow_mut().insert(vk);
            false
        });
        let expect: std::collections::BTreeSet<i32> =
            [0x12, 0x5B, 0x5C, 0x11, 0x10, 0x48].into_iter().collect();
        assert_eq!(*seen.borrow(), expect, "必須探測 Alt/LWin/RWin/Ctrl/Shift/H 全部六鍵");
    }

    #[test]
    fn wait_for_modifier_release_polls_until_keys_released() {
        // 前 3 次探測回報「按住」,之後全部放開 — 必須輪詢直到放開才返回
        let calls = std::cell::Cell::new(0u32);
        let start = std::time::Instant::now();
        wait_for_modifier_release(|_vk| {
            calls.set(calls.get() + 1);
            calls.get() < 4
        });
        assert!(calls.get() >= 4, "放開後才返回(實際探測 {} 次)", calls.get());
        assert!(start.elapsed() < std::time::Duration::from_secs(1), "不應撞到 1s 上限");
    }

    #[test]
    fn wait_for_modifier_release_caps_at_one_second_when_never_released() {
        let start = std::time::Instant::now();
        wait_for_modifier_release(|_| true);
        let elapsed = start.elapsed();
        assert!(elapsed >= std::time::Duration::from_secs(1), "永遠按住 → 等滿 1s 上限");
        assert!(elapsed < std::time::Duration::from_secs(5), "上限後必須立即返回");
    }
}
