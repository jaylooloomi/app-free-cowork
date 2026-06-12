//! Synthesizes Win+H to invoke Windows 11 built-in voice typing into the
//! currently-focused edit field (the palette input). Cloud-based (Azure),
//! zh-TW supported; requires the OS "online speech recognition" setting.

#[cfg(windows)]
use windows::Win32::UI::Input::KeyboardAndMouse::{
    SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP,
    VIRTUAL_KEY, VK_LWIN,
};

/// 'H' virtual-key code (no named constant in the windows crate for letters).
#[cfg(windows)]
const VK_H: VIRTUAL_KEY = VIRTUAL_KEY(0x48);

/// The Win+H chord as (virtual key, is_key_up) pairs, in send order.
/// Kept as data so the sequence is unit-testable without touching SendInput.
#[cfg(windows)]
const WIN_H_SEQUENCE: [(VIRTUAL_KEY, bool); 4] =
    [(VK_LWIN, false), (VK_H, false), (VK_H, true), (VK_LWIN, true)];

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

/// Pure builder: LWIN down, H down, H up, LWIN up.
#[cfg(windows)]
pub fn build_win_h_inputs() -> [INPUT; 4] {
    WIN_H_SEQUENCE.map(|(vk, up)| key_input(vk, up))
}

/// Sends the Win+H chord to the foreground window (i.e. opens voice typing).
/// Untestable-thin by design: all logic lives in `build_win_h_inputs`.
#[cfg(windows)]
pub fn trigger_voice_typing() {
    let inputs = build_win_h_inputs();
    // SAFETY: `inputs` is a valid, properly-initialized INPUT array and the
    // size argument matches the element type, per the SendInput contract.
    unsafe {
        SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
    }
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
}
