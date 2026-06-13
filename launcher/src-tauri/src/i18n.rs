//! 後端使用者可見字串的本地化(繁體中文預設 + English)。
//!
//! 設計:一個 process-global 的目前語系(`AtomicU8`,zh=0 / en=1),由
//! `set_locale` 在每個 tauri 指令進入點以使用者設定更新一次。每一條使用者
//! 可見訊息對應一個小函式,內部 `match current()` 回傳對應語系字串。前端不需
//! 介入 —— 設定一變更(save_settings)或任何指令進入時都會刷新語系,因此訊息
//! 永遠以使用者當下選定的語言呈現。
//!
//! 注意:這是 process-global 狀態。測試若要驗證 En 字串,請在「同一個 `#[test]`」
//! 內 set + assert,避免與其他測試的 zh 預設互相干擾。

use std::sync::atomic::{AtomicU8, Ordering};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Locale {
    ZhTw,
    En,
}

/// Process-global 目前語系;0 = 繁中(預設)、1 = English。
static LOCALE: AtomicU8 = AtomicU8::new(0);

/// 由設定字串解析並設定目前語系:"en"(不分大小寫)→ English,其餘 → 繁中。
/// 未知語系一律回退繁中(預設語言)。
pub fn set_locale(s: &str) {
    let loc = if s.eq_ignore_ascii_case("en") { Locale::En } else { Locale::ZhTw };
    LOCALE.store(loc as u8, Ordering::Relaxed);
}

/// 目前 process-global 語系。
pub fn current() -> Locale {
    match LOCALE.load(Ordering::Relaxed) {
        1 => Locale::En,
        _ => Locale::ZhTw,
    }
}

// ─────────────────────────── ipc.rs: get_status ───────────────────────────

/// claude 路徑:已安裝但尚未登入 Anthropic。
pub fn claude_not_logged_in() -> String {
    match current() {
        Locale::ZhTw => "尚未登入 Anthropic — 首次執行會在終端機要求 /login".into(),
        Locale::En => "Not signed in to Anthropic — the first run will prompt /login in the terminal".into(),
    }
}

/// claude 狀態無法判定(理論上不會發生的保底分支)。
pub fn claude_status_unknown() -> String {
    match current() {
        Locale::ZhTw => "Claude 狀態未知".into(),
        Locale::En => "Claude status unknown".into(),
    }
}

/// 雲端目錄抓不到 → 離線。
pub fn offline_cloud_needs_network() -> String {
    match current() {
        Locale::ZhTw => "離線 — 雲端模型需要網路連線".into(),
        Locale::En => "Offline — cloud models require a network connection".into(),
    }
}

/// 首次使用尚需安裝元件。
pub fn first_run_will_install() -> String {
    match current() {
        Locale::ZhTw => "首次使用將自動安裝必要元件".into(),
        Locale::En => "Required components will be installed automatically on first use".into(),
    }
}

// ─────────────────────────── ipc.rs: submit_prompt / queue ───────────────────────────

/// 送出空白需求。
pub fn empty_prompt() -> String {
    match current() {
        Locale::ZhTw => "請輸入需求".into(),
        Locale::En => "Please enter a request".into(),
    }
}

/// 佇列接續時的「開始執行」通知(帶需求預覽)。
pub fn task_starting(preview: &str) -> String {
    match current() {
        Locale::ZhTw => format!("開始執行:{preview}…"),
        Locale::En => format!("Starting: {preview}…"),
    }
}

/// 無法建立執行記錄檔。
pub fn log_create_failed(err: &str) -> String {
    match current() {
        Locale::ZhTw => format!("無法建立記錄檔:{err}"),
        Locale::En => format!("Could not create log file: {err}"),
    }
}

/// spawn 啟動失敗。
pub fn launch_failed(err: &str) -> String {
    match current() {
        Locale::ZhTw => format!("啟動失敗:{err}"),
        Locale::En => format!("Launch failed: {err}"),
    }
}

// ─────────────────────────── ipc.rs: handle_task_exit ───────────────────────────

/// 使用者主動停止後的通知。
pub fn task_stopped() -> String {
    match current() {
        Locale::ZhTw => "已停止任務".into(),
        Locale::En => "Task stopped".into(),
    }
}

/// 背景任務正常完成。
pub fn task_done() -> String {
    match current() {
        Locale::ZhTw => "任務完成".into(),
        Locale::En => "Task complete".into(),
    }
}

/// 前景任務啟動後立即失敗、且無記錄輸出可分類。
pub fn fast_fail(code: i32) -> String {
    match current() {
        Locale::ZhTw => format!("啟動後立即失敗 (exit {code}),可能是模型需訂閱、額度用盡或登入失效"),
        Locale::En => {
            format!("Failed immediately after launch (exit {code}); the model may require a subscription, the quota may be exhausted, or the sign-in may have expired")
        }
    }
}

/// 失敗分類:此模型需要付費訂閱。
pub fn subscription_required() -> String {
    match current() {
        Locale::ZhTw => "此模型需要付費訂閱 — 請到設定改用免費模型(如 minimax-m2.5:cloud)".into(),
        Locale::En => "This model requires a paid subscription — switch to a free model in Settings (e.g. minimax-m2.5:cloud)".into(),
    }
}

/// 失敗分類:免費額度用盡。
pub fn quota_exhausted() -> String {
    match current() {
        Locale::ZhTw => "免費額度已用完,稍後重置(限制綁帳號,換模型無效)".into(),
        Locale::En => "Free quota exhausted; it resets later (the limit is per-account, so switching models won't help)".into(),
    }
}

/// 失敗分類:需要重新登入 ollama.com。
pub fn need_relogin() -> String {
    match current() {
        Locale::ZhTw => "需要重新登入 ollama.com,下次啟動會自動引導".into(),
        Locale::En => "You need to sign in to ollama.com again; the next launch will guide you through it".into(),
    }
}

/// 失敗分類:任務異常結束(原因未知,code == -1)。
pub fn task_crashed(log_path: &str) -> String {
    match current() {
        Locale::ZhTw => format!("任務異常結束(原因未知),記錄:{log_path}"),
        Locale::En => format!("Task ended abnormally (cause unknown); log: {log_path}"),
    }
}

/// 失敗分類:任務以非零碼結束。
pub fn task_failed(code: i32, log_path: &str) -> String {
    match current() {
        Locale::ZhTw => format!("任務失敗 (exit {code}),記錄:{log_path}"),
        Locale::En => format!("Task failed (exit {code}); log: {log_path}"),
    }
}

// ─────────────────────────── ipc.rs: wizard_run ───────────────────────────

/// Ollama 服務尚未就緒(signin/model 步驟前置檢查失敗)。
pub fn ollama_not_ready() -> String {
    match current() {
        Locale::ZhTw => "Ollama 服務尚未就緒,請重試".into(),
        Locale::En => "The Ollama service is not ready yet; please retry".into(),
    }
}

/// 未知的精靈步驟。
pub fn unknown_step(step: &str) -> String {
    match current() {
        Locale::ZhTw => format!("未知步驟 {step}"),
        Locale::En => format!("Unknown step {step}"),
    }
}

// ─────────────────────────── ipc.rs: queue_cancel / task_stop ───────────────────────────

/// 佇列中找不到要取消的任務。
pub fn task_not_in_queue() -> String {
    match current() {
        Locale::ZhTw => "佇列中找不到該任務".into(),
        Locale::En => "That task was not found in the queue".into(),
    }
}

/// 目前沒有執行中的任務。
pub fn no_running_task() -> String {
    match current() {
        Locale::ZhTw => "目前沒有執行中的任務".into(),
        Locale::En => "There is no task currently running".into(),
    }
}

/// 任務已結束(換手 / id 不符)。
pub fn task_already_ended() -> String {
    match current() {
        Locale::ZhTw => "任務已結束".into(),
        Locale::En => "The task has already ended".into(),
    }
}

/// 前景任務無法以 task_stop 停止。
pub fn foreground_close_terminal() -> String {
    match current() {
        Locale::ZhTw => "前景任務請直接關閉其終端機視窗".into(),
        Locale::En => "For a foreground task, close its terminal window directly".into(),
    }
}

/// 無法取得任務的 PID。
pub fn no_task_pid() -> String {
    match current() {
        Locale::ZhTw => "無法取得任務的處理程序 ID".into(),
        Locale::En => "Could not obtain the task's process ID".into(),
    }
}

// ─────────────────────────── ipc.rs: set_model ───────────────────────────

/// 模型名稱不可為空。
pub fn empty_model_name() -> String {
    match current() {
        Locale::ZhTw => "模型名稱不可為空".into(),
        Locale::En => "The model name cannot be empty".into(),
    }
}

// ─────────────────────────── ipc.rs: scan_models ───────────────────────────

/// 掃描時 Ollama 服務未回應。
pub fn ollama_not_responding_for_scan() -> String {
    match current() {
        Locale::ZhTw => "Ollama 服務未回應,請先確認 Ollama 已啟動後再掃描".into(),
        Locale::En => "The Ollama service is not responding; make sure Ollama is running before scanning".into(),
    }
}

// ─────────────────────────── ipc.rs: open_url ───────────────────────────

/// 不在白名單的網址。
pub fn url_not_allowed() -> String {
    match current() {
        Locale::ZhTw => "不允許開啟此網址".into(),
        Locale::En => "Opening this URL is not allowed".into(),
    }
}

// ─────────────────────────── ipc.rs: start_voice_input ───────────────────────────

/// 找不到輸入面板視窗。
pub fn palette_window_not_found() -> String {
    match current() {
        Locale::ZhTw => "找不到輸入面板視窗".into(),
        Locale::En => "Could not find the input palette window".into(),
    }
}

/// 無法聚焦輸入面板。
pub fn palette_focus_failed() -> String {
    match current() {
        Locale::ZhTw => "無法聚焦輸入面板".into(),
        Locale::En => "Could not focus the input palette".into(),
    }
}

// ─────────────────────────── doctor.rs ───────────────────────────

/// claude 尚未安裝。
pub fn claude_not_installed() -> String {
    match current() {
        Locale::ZhTw => "尚未安裝 Claude Code(請先完成首次安裝)".into(),
        Locale::En => "Claude Code is not installed yet (please complete the first-time install)".into(),
    }
}

/// Ollama 服務未回應(體檢時)。
pub fn ollama_service_down() -> String {
    match current() {
        Locale::ZhTw => "Ollama 服務未回應，請重新啟動 Ollama 後再試".into(),
        Locale::En => "The Ollama service is not responding; restart Ollama and try again".into(),
    }
}

// ─────────────────────────── bootstrap.rs ───────────────────────────
//
// StepResult.detail 的動作標籤 + 結果片語。原本 bootstrap.rs 以
// `format!("{action} 完成")` / `format!("{action} 失敗 ...")` 組裝;這裡提供
// 動作標籤(action_*)與結果包裝(step_ok / step_failed_with_tail /
// step_failed_err)讓整段訊息可本地化。

/// 動作標籤:安裝 Ollama(winget)。
pub fn action_install_ollama_winget() -> String {
    match current() {
        Locale::ZhTw => "安裝 Ollama (winget)".into(),
        Locale::En => "Install Ollama (winget)".into(),
    }
}

/// 動作標籤:安裝 Ollama(直接下載)。
pub fn action_install_ollama_direct() -> String {
    match current() {
        Locale::ZhTw => "安裝 Ollama (直接下載)".into(),
        Locale::En => "Install Ollama (direct download)".into(),
    }
}

/// 動作標籤:安裝 Claude Code。
pub fn action_install_claude() -> String {
    match current() {
        Locale::ZhTw => "安裝 Claude Code".into(),
        Locale::En => "Install Claude Code".into(),
    }
}

/// 動作標籤:登入 ollama.com。
pub fn action_signin() -> String {
    match current() {
        Locale::ZhTw => "登入 ollama.com".into(),
        Locale::En => "Sign in to ollama.com".into(),
    }
}

/// 動作標籤:註冊雲端模型。
pub fn action_register_model() -> String {
    match current() {
        Locale::ZhTw => "註冊雲端模型".into(),
        Locale::En => "Register cloud model".into(),
    }
}

/// 步驟成功:`{action} 完成`。
pub fn step_ok(action: &str) -> String {
    match current() {
        Locale::ZhTw => format!("{action} 完成"),
        Locale::En => format!("{action} complete"),
    }
}

/// 步驟失敗(帶 exit code + 輸出尾段)。
pub fn step_failed_with_tail(action: &str, code: i32, tail: &str) -> String {
    match current() {
        Locale::ZhTw => format!("{action} 失敗 (exit {code}): {tail}"),
        Locale::En => format!("{action} failed (exit {code}): {tail}"),
    }
}

/// 步驟失敗(IO/spawn 錯誤)。
pub fn step_failed_err(action: &str, err: &str) -> String {
    match current() {
        Locale::ZhTw => format!("{action} 失敗: {err}"),
        Locale::En => format!("{action} failed: {err}"),
    }
}

/// 下載 OllamaSetup.exe 失敗。
pub fn download_setup_failed(err: &str) -> String {
    match current() {
        Locale::ZhTw => format!("下載 OllamaSetup.exe 失敗: {err}"),
        Locale::En => format!("Failed to download OllamaSetup.exe: {err}"),
    }
}

/// winget 執行失敗(spawn 層級)。
pub fn winget_run_failed(err: &str) -> String {
    match current() {
        Locale::ZhTw => format!("winget 執行失敗: {err}"),
        Locale::En => format!("winget failed to run: {err}"),
    }
}

/// 直接下載也失敗時,把先前 winget 失敗原因附在後面。
pub fn fallback_with_prior(detail: &str, note: &str) -> String {
    match current() {
        Locale::ZhTw => format!("{detail}(先前 {note})"),
        Locale::En => format!("{detail} (previously: {note})"),
    }
}

/// 登入流程已在進行中。
pub fn signin_in_progress() -> String {
    match current() {
        Locale::ZhTw => "登入流程已在進行中,請先完成瀏覽器配對".into(),
        Locale::En => "A sign-in is already in progress; finish the browser pairing first".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 全域語系狀態:在單一測試內 set + assert(en → 中 → 未知回退中),
    /// 避免與其他測試的 zh 預設互相競爭。結束時還原為預設繁中。
    #[test]
    fn set_locale_switches_language_and_defaults_to_zh() {
        // English
        set_locale("en");
        assert_eq!(current(), Locale::En);
        assert_eq!(empty_prompt(), "Please enter a request");
        assert_eq!(task_done(), "Task complete");
        assert_eq!(fast_fail(7), "Failed immediately after launch (exit 7); the model may require a subscription, the quota may be exhausted, or the sign-in may have expired");
        assert_eq!(task_failed(2, r"C:\log"), "Task failed (exit 2); log: C:\\log");
        assert_eq!(step_ok("Install Claude Code"), "Install Claude Code complete");

        // case-insensitive "EN"
        set_locale("EN");
        assert_eq!(current(), Locale::En);

        // 繁中
        set_locale("zh-TW");
        assert_eq!(current(), Locale::ZhTw);
        assert_eq!(empty_prompt(), "請輸入需求");
        assert_eq!(task_done(), "任務完成");
        assert_eq!(foreground_close_terminal(), "前景任務請直接關閉其終端機視窗");

        // 未知語系 → 回退繁中(預設)
        set_locale("fr");
        assert_eq!(current(), Locale::ZhTw);
        assert_eq!(empty_prompt(), "請輸入需求");

        // 還原預設(避免汙染其他測試的 zh 斷言)
        set_locale("zh-TW");
        assert_eq!(current(), Locale::ZhTw);
    }
}
