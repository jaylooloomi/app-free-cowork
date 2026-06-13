use crate::settings::Settings;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq)]
pub struct LaunchSpec {
    pub program: String,
    pub args: Vec<String>,
    pub cwd: PathBuf,
    pub background: bool,
    /// 額外環境變數(spawn 時套用)。Ollama 路徑會設 CLAUDE_CODE_MAX_OUTPUT_TOKENS
    /// 上限,避免小模型(輸出上限低於 Claude Code 預設 32000)回 400。
    pub env: Vec<(String, String)>,
}

/// 安全的輸出上限:Claude Code 預設一次要 32000 output tokens,許多開源模型上限
/// 較低(實測 rnj-1:8b 僅 16384)。設此上限讓「只是輸出上限低」的模型可用,且
/// 對大模型無害(實測 minimax-m2.5 設 16384 仍正常)。只用於 Ollama 路徑;
/// 真 Claude(Anthropic 帳號)不設限,保留完整能力。
const OLLAMA_MAX_OUTPUT_TOKENS: &str = "16384";

/// claude 的執行檔路徑:優先 `%USERPROFILE%\.local\bin\claude.exe`,否則靠 PATH。
fn claude_program() -> String {
    if let Some(home) = dirs::home_dir() {
        let p = home.join(".local").join("bin").join("claude.exe");
        if p.exists() {
            return p.to_string_lossy().into_owned();
        }
    }
    "claude".into()
}

/// 權限旗標:謹慎模式 → acceptEdits;否則 → dangerously-skip-permissions。
fn permission_args(s: &Settings) -> Vec<String> {
    if s.cautious_mode {
        vec!["--permission-mode".into(), "acceptEdits".into()]
    } else {
        vec!["--dangerously-skip-permissions".into()]
    }
}

/// 「動手型助手」系統提示:讓模型直接執行使用者要求,而不是只給指令或反問。
/// 實測 Claude/Opus 預設偏向解釋;附加這段後會直接執行(如 Start-Process 開 App/網頁)。
/// 危險操作仍由權限機制把關,所以兩種模式都附加。依介面語言給中/英。
fn agent_system_prompt(locale: &str) -> &'static str {
    if locale.eq_ignore_ascii_case("en") {
        "You are a do-it desktop assistant on Windows. The user wants tasks DONE, not explained. \
         You can run any PowerShell/Bash command and operate files. For requests like \"open X\", \
         run it directly (e.g. Start-Process for apps/URLs) instead of telling the user how. \
         Don't ask for confirmation and don't just give instructions — perform the action and \
         report the result in one short line. (Risky operations are still gated by the permission system.)"
    } else {
        "你是 Windows 上的「動手型」桌面助手。使用者要的是把事情做完,不是教學或反問。\
         你可以執行任何 PowerShell/Bash 指令、操作檔案。遇到「開啟 X」這類要求就直接執行\
         (例如用 Start-Process 開啟應用程式或網址),不要只告訴使用者怎麼做。\
         不要反問確認、不要只給指令 — 直接動手完成,並用一句話回報結果。\
         (危險操作仍會由權限機制把關。)"
    }
}

/// 把系統提示插在 claude 參數最前面:`--append-system-prompt <文字>`。
/// 使用者自訂(settings.system_prompt 非空)優先,否則用內建的語言預設。
fn system_prompt_args(s: &Settings) -> Vec<String> {
    let text: String = if s.system_prompt.trim().is_empty() {
        agent_system_prompt(&s.locale).into()
    } else {
        s.system_prompt.clone()
    };
    vec!["--append-system-prompt".into(), text]
}

/// 背景模式追加的 claude 參數:`-p` 啟用 print(非互動)模式;`--output-format
/// stream-json --verbose` 讓 claude 逐行吐出 JSONL 事件,供面板即時解析顯示。
/// stream-json 是 claude CLI 自己的輸出格式,與底層模型無關,故 ollama 透傳路徑
/// (`ollama launch claude -- …`)同樣適用。
fn background_args() -> Vec<String> {
    vec![
        "-p".into(),
        "--output-format".into(),
        "stream-json".into(),
        "--verbose".into(),
    ]
}

pub fn build_launch_spec(prompt: &str, s: &Settings, model: &str) -> LaunchSpec {
    // claude 哨符 → 直接跑 claude(用 Anthropic 帳號),不經 ollama、不帶 --model、
    // 不設 Ollama 環境變數。前景 = 互動;背景 = -p。
    if model == crate::catalog::CLAUDE_MODEL {
        let mut args: Vec<String> = system_prompt_args(s);
        if s.background_mode {
            args.extend(background_args());
        }
        args.extend(permission_args(s));
        args.push(prompt.to_string());
        return LaunchSpec {
            program: claude_program(),
            args,
            cwd: s.effective_working_dir(),
            background: s.background_mode,
            env: Vec::new(), // 真 Claude 不限制輸出
        };
    }

    let mut args: Vec<String> = vec![
        "launch".into(),
        "claude".into(),
        "--model".into(),
        model.into(),
        "--yes".into(),
        "--".into(),
    ];
    args.extend(system_prompt_args(s));
    if s.background_mode {
        args.extend(background_args());
    }
    args.extend(permission_args(s));
    args.push(prompt.to_string());
    LaunchSpec {
        program: "ollama".into(),
        args,
        cwd: s.effective_working_dir(),
        background: s.background_mode,
        env: vec![("CLAUDE_CODE_MAX_OUTPUT_TOKENS".into(), OLLAMA_MAX_OUTPUT_TOKENS.into())],
    }
}

#[cfg(windows)]
const CREATE_NEW_CONSOLE: u32 = 0x0000_0010;
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[derive(Debug, PartialEq)]
pub enum FailureKind {
    /// Model is gated behind a paid Ollama plan (403 "requires a subscription").
    Subscription,
    Quota,
    Auth,
    Other,
}

pub fn classify_failure(log_tail: &str) -> FailureKind {
    let t = log_tail.to_lowercase();
    // Subscription check first: "requires a subscription" is more specific than
    // the generic quota/auth tokens (verified live 2026-06-12 with minimax-m2.7).
    if t.contains("requires a subscription") || t.contains("403") {
        FailureKind::Subscription
    } else if t.contains("429") || t.contains("usage limit") || t.contains("quota") || t.contains("rate limit") {
        FailureKind::Quota
    } else if t.contains("401") || t.contains("unauthorized") || t.contains("not signed in") || t.contains("signin") {
        FailureKind::Auth
    } else {
        FailureKind::Other
    }
}

/// Waiter callback: (exit code, log path, elapsed since spawn).
pub type OnDone = Box<dyn FnOnce(i32, PathBuf, std::time::Duration) + Send>;

/// Per-stdout-line callback for streaming (background) launches. Called once per
/// line of the child's stdout (one stream-json JSONL record), in order, on a
/// reader thread. Used by ipc to forward output to the palette in real time.
pub type OnLine = Box<dyn Fn(&str) + Send>;

/// Spawn contract (v1.1):
/// - Returns the child PID (used by `task_stop` to `taskkill` background tasks).
/// - For BOTH modes the waiter thread ALWAYS calls `on_done(code, log_path, elapsed)`
///   when the process exits — task-queue chaining needs to observe every exit.
///   Notification policy (e.g. "foreground fast-fail", "stay silent when the user
///   closes a long-running console") lives in the caller (ipc), which decides
///   using `code`, `elapsed` and the launch mode.
/// - Foreground has no real log; log_path is passed through so callers can
///   construct messages uniformly (the file will be empty / non-existent).
/// `on_line` (streaming): when provided AND the launch is background, stdout is
/// piped and read line-by-line — each line is appended to the log AND handed to
/// `on_line` (in order). stderr is drained to the same log. This powers the live
/// stream-json display in the palette. When `on_line` is None, background falls
/// back to the simple "redirect both streams to the log file" path.
pub fn spawn(
    spec: &LaunchSpec,
    log_path: PathBuf,
    on_done: Option<OnDone>,
    on_line: Option<OnLine>,
) -> std::io::Result<u32> {
    use std::os::windows::process::CommandExt;
    use std::process::{Command, Stdio};
    let mut cmd = Command::new(&spec.program);
    cmd.args(&spec.args).current_dir(&spec.cwd);
    for (k, v) in &spec.env {
        cmd.env(k, v);
    }

    // 串流路徑:背景 + 有逐行回呼 → pipe stdout/stderr,逐行轉送並 tee 進 log。
    if spec.background {
        if let Some(on_line) = on_line {
            return spawn_streaming(cmd, log_path, on_done, on_line);
        }
        let log = std::fs::File::create(&log_path)?;
        let log2 = log.try_clone()?;
        cmd.creation_flags(CREATE_NO_WINDOW)
            .stdin(Stdio::null())
            .stdout(log)
            .stderr(log2);
    } else {
        cmd.creation_flags(CREATE_NEW_CONSOLE);
    }
    let start = std::time::Instant::now();
    let mut child = cmd.spawn()?;
    let pid = child.id();
    std::thread::spawn(move || {
        let code = child.wait().map(|s| s.code().unwrap_or(-1)).unwrap_or(-1);
        if let Some(f) = on_done {
            f(code, log_path, start.elapsed());
        }
    });
    Ok(pid)
}

/// Background streaming spawn: stdout piped → per-line `on_line` + tee to log;
/// stderr piped → tee to the same log (shared behind a Mutex so the two reader
/// threads never interleave-corrupt the file). The waiter joins both readers
/// before firing `on_done`, so by completion the log is fully written and every
/// line has been delivered.
fn spawn_streaming(
    mut cmd: std::process::Command,
    log_path: PathBuf,
    on_done: Option<OnDone>,
    on_line: OnLine,
) -> std::io::Result<u32> {
    use std::io::{BufRead, BufReader, Write};
    use std::os::windows::process::CommandExt;
    use std::panic::{catch_unwind, AssertUnwindSafe};
    use std::process::Stdio;
    use std::sync::{Arc, Mutex};

    cmd.creation_flags(CREATE_NO_WINDOW)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let start = std::time::Instant::now();
    let mut child = cmd.spawn()?;
    let pid = child.id();
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let log = Arc::new(Mutex::new(std::fs::File::create(&log_path)?));

    // 即使 mutex 曾因 panic 中毒(into_inner 取回守衛),仍持續寫 log:log 尾巴是
    // 失敗分類(classify_failure)的唯一依據,絕不能因一次例外而靜默截斷。
    fn append(log: &Mutex<std::fs::File>, line: &str) {
        let mut f = log.lock().unwrap_or_else(|e| e.into_inner());
        let _ = writeln!(f, "{line}");
    }

    let log_out = log.clone();
    let out_handle = std::thread::spawn(move || {
        if let Some(out) = stdout {
            for line in BufReader::new(out).lines().map_while(Result::ok) {
                append(&log_out, &line);
                // on_line 會呼叫 Tauri emit;包 catch_unwind,單行回呼即使 panic
                // 也不會中斷整個 stdout 讀取迴圈(否則後續輸出與 log 全部遺失)。
                let _ = catch_unwind(AssertUnwindSafe(|| on_line(&line)));
            }
        }
    });
    let log_err = log.clone();
    let err_handle = std::thread::spawn(move || {
        if let Some(err) = stderr {
            for line in BufReader::new(err).lines().map_while(Result::ok) {
                append(&log_err, &line);
            }
        }
    });

    std::thread::spawn(move || {
        let code = child.wait().map(|s| s.code().unwrap_or(-1)).unwrap_or(-1);
        let _ = out_handle.join();
        let _ = err_handle.join();
        if let Some(f) = on_done {
            f(code, log_path, start.elapsed());
        }
    });
    Ok(pid)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::Settings;

    #[test]
    fn classifies_runtime_errors_from_log_tail() {
        assert_eq!(classify_failure("... 429 Too Many Requests ..."), FailureKind::Quota);
        assert_eq!(classify_failure("...usage limit reached..."), FailureKind::Quota);
        assert_eq!(classify_failure("... 401 Unauthorized ..."), FailureKind::Auth);
        assert_eq!(classify_failure("...not signed in..."), FailureKind::Auth);
        assert_eq!(
            classify_failure("API Error: 403 this model requires a subscription, upgrade for access"),
            FailureKind::Subscription
        );
        assert_eq!(classify_failure("random crash"), FailureKind::Other);
    }

    #[test]
    fn foreground_fast_fail_calls_on_done_with_elapsed() {
        use std::sync::mpsc;
        let dir = tempfile::tempdir().unwrap();
        // log_path for foreground is unused (file may not exist) — pass a path in a writable dir
        let log = dir.path().join("fg.log");
        let spec = LaunchSpec {
            program: "cmd".into(),
            args: vec!["/c".into(), "exit 7".into()],
            cwd: dir.path().to_path_buf(),
            background: false,
            env: Vec::new(),
        };
        let (tx, rx) = mpsc::channel();
        spawn(&spec, log.clone(), Some(Box::new(move |code, path, elapsed| {
            tx.send((code, path, elapsed)).unwrap();
        })), None).unwrap();
        // cmd /c exit 7 exits in <<1 second — caller would classify this as fast-fail
        let (code, _path, elapsed) = rx.recv_timeout(std::time::Duration::from_secs(15)).unwrap();
        assert_eq!(code, 7);
        assert!(elapsed < std::time::Duration::from_secs(15), "elapsed should reflect actual runtime; got {elapsed:?}");
    }

    #[test]
    fn foreground_normal_exit_calls_on_done() {
        use std::sync::mpsc;
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("fg-ok.log");
        let spec = LaunchSpec {
            program: "cmd".into(),
            args: vec!["/c".into(), "exit 0".into()],
            cwd: dir.path().to_path_buf(),
            background: false,
            env: Vec::new(),
        };
        let (tx, rx) = mpsc::channel();
        let pid = spawn(&spec, log, Some(Box::new(move |code, _path, elapsed| {
            tx.send((code, elapsed)).unwrap();
        })), None).unwrap();
        assert!(pid > 0, "spawn must return the child PID");
        // v1.1 contract: waiter fires on EVERY exit (queue chaining), even foreground code 0
        let (code, elapsed) = rx.recv_timeout(std::time::Duration::from_secs(15)).unwrap();
        assert_eq!(code, 0);
        assert!(elapsed < std::time::Duration::from_secs(15));
    }

    #[test]
    fn claude_model_runs_claude_directly_not_ollama() {
        // 前景:claude "<prompt>" + 權限旗標,完全不經 ollama / --model
        let s = Settings { background_mode: false, ..Default::default() };
        let spec = build_launch_spec("看這張圖", &s, crate::catalog::CLAUDE_MODEL);
        assert_ne!(spec.program, "ollama");
        assert!(
            spec.program == "claude" || spec.program.ends_with("claude.exe"),
            "program should be the claude binary, got {}",
            spec.program
        );
        assert_eq!(
            spec.args,
            vec!["--append-system-prompt", agent_system_prompt("zh-TW"), "--dangerously-skip-permissions", "看這張圖"]
        );
        assert!(!spec.args.iter().any(|a| a == "launch" || a == "--model"));
        // 真 Claude 不限制輸出
        assert!(spec.env.is_empty());
    }

    #[test]
    fn custom_system_prompt_overrides_default() {
        // 預設(空)→ 用內建語言預設
        let spec = build_launch_spec("p", &Settings::default(), "minimax-m2.5:cloud");
        let i = spec.args.iter().position(|a| a == "--append-system-prompt").unwrap();
        assert_eq!(spec.args[i + 1], agent_system_prompt("zh-TW"));
        // 自訂 → 用使用者的文字
        let s = Settings { system_prompt: "只用注音回答".into(), ..Default::default() };
        let spec = build_launch_spec("p", &s, "minimax-m2.5:cloud");
        let i = spec.args.iter().position(|a| a == "--append-system-prompt").unwrap();
        assert_eq!(spec.args[i + 1], "只用注音回答");
    }

    #[test]
    fn ollama_path_caps_max_output_tokens() {
        // 小模型(輸出上限 < 32000)會 400;Ollama 路徑統一設 16384 上限
        let spec = build_launch_spec("p", &Settings::default(), "minimax-m2.5:cloud");
        assert!(spec
            .env
            .iter()
            .any(|(k, v)| k == "CLAUDE_CODE_MAX_OUTPUT_TOKENS" && v == "16384"));
    }

    #[test]
    fn claude_model_background_and_cautious() {
        let sys = agent_system_prompt("zh-TW");
        let bg = Settings { background_mode: true, ..Default::default() };
        let spec = build_launch_spec("p", &bg, crate::catalog::CLAUDE_MODEL);
        assert_eq!(
            spec.args,
            vec![
                "--append-system-prompt", sys,
                "-p", "--output-format", "stream-json", "--verbose",
                "--dangerously-skip-permissions", "p"
            ]
        );
        assert!(spec.background);

        let cautious = Settings { cautious_mode: true, background_mode: false, ..Default::default() };
        let spec = build_launch_spec("p", &cautious, crate::catalog::CLAUDE_MODEL);
        assert_eq!(spec.args, vec!["--append-system-prompt", sys, "--permission-mode", "acceptEdits", "p"]);
    }

    #[test]
    fn foreground_args() {
        // 前景模式(非預設):無 -p、無 stream-json,直接互動式啟動
        let s = Settings { background_mode: false, ..Default::default() };
        let spec = build_launch_spec("整理 \"桌面\" 並分類", &s, "minimax-m2.7:cloud");
        assert_eq!(spec.program, "ollama");
        assert_eq!(
            spec.args,
            vec![
                "launch",
                "claude",
                "--model",
                "minimax-m2.7:cloud",
                "--yes",
                "--",
                "--append-system-prompt",
                agent_system_prompt("zh-TW"),
                "--dangerously-skip-permissions",
                "整理 \"桌面\" 並分類"
            ]
        );
        assert!(!spec.background);
    }

    #[test]
    fn cautious_mode_swaps_permission_flag() {
        let s = Settings { cautious_mode: true, background_mode: false, ..Default::default() };
        let spec = build_launch_spec("p", &s, "m");
        assert_eq!(
            spec.args,
            vec![
                "launch",
                "claude",
                "--model",
                "m",
                "--yes",
                "--",
                "--append-system-prompt",
                agent_system_prompt("zh-TW"),
                "--permission-mode",
                "acceptEdits",
                "p"
            ]
        );
    }

    #[test]
    fn background_mode_adds_streaming_flags() {
        let s = Settings { background_mode: true, ..Default::default() };
        let spec = build_launch_spec("p", &s, "m");
        assert_eq!(
            spec.args,
            vec![
                "launch",
                "claude",
                "--model",
                "m",
                "--yes",
                "--",
                "--append-system-prompt",
                agent_system_prompt("zh-TW"),
                "-p",
                "--output-format",
                "stream-json",
                "--verbose",
                "--dangerously-skip-permissions",
                "p"
            ]
        );
        assert!(spec.background);
    }

    #[test]
    fn background_spawn_redirects_output_and_reports_exit_code() {
        use std::sync::mpsc;
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("run.log");
        let spec = LaunchSpec {
            program: "cmd".into(),
            args: vec!["/c".into(), "echo out-line & echo err-line 1>&2 & exit 3".into()],
            cwd: dir.path().to_path_buf(),
            background: true,
            env: Vec::new(),
        };
        let (tx, rx) = mpsc::channel();
        spawn(&spec, log.clone(), Some(Box::new(move |code, path, _elapsed| { tx.send((code, path)).unwrap(); })), None).unwrap();
        let (code, path) = rx.recv_timeout(std::time::Duration::from_secs(15)).unwrap();
        assert_eq!(code, 3);
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("out-line"), "log missing stdout; got: {content}");
        assert!(content.contains("err-line"), "log missing stderr; got: {content}");
    }

    #[test]
    fn streaming_spawn_delivers_lines_and_tees_to_log() {
        use std::sync::mpsc;
        use std::sync::{Arc, Mutex};
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("stream.log");
        let spec = LaunchSpec {
            program: "cmd".into(),
            args: vec!["/c".into(), "echo line-a & echo line-b & echo err-x 1>&2 & exit 0".into()],
            cwd: dir.path().to_path_buf(),
            background: true,
            env: Vec::new(),
        };
        let lines = Arc::new(Mutex::new(Vec::<String>::new()));
        let lines2 = lines.clone();
        let (tx, rx) = mpsc::channel();
        spawn(
            &spec,
            log.clone(),
            Some(Box::new(move |code, path, _| { tx.send((code, path)).unwrap(); })),
            Some(Box::new(move |line: &str| { lines2.lock().unwrap().push(line.to_string()); })),
        )
        .unwrap();
        // on_done fires only after the reader threads join → lines/log are complete here.
        let (code, path) = rx.recv_timeout(std::time::Duration::from_secs(15)).unwrap();
        assert_eq!(code, 0);
        let captured = lines.lock().unwrap().clone();
        assert!(captured.iter().any(|l| l.contains("line-a")), "on_line missing line-a; got {captured:?}");
        assert!(captured.iter().any(|l| l.contains("line-b")), "on_line missing line-b; got {captured:?}");
        // stderr is NOT delivered to on_line (stdout only)…
        assert!(!captured.iter().any(|l| l.contains("err-x")), "on_line must only see stdout; got {captured:?}");
        // …but BOTH streams are tee'd into the log for failure classification.
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("line-a") && content.contains("err-x"), "log missing tee'd output; got {content}");
    }

    #[test]
    fn working_dir_defaults_to_home() {
        let spec = build_launch_spec("p", &Settings::default(), "m");
        assert_eq!(spec.cwd, dirs::home_dir().unwrap());
        let dir = tempfile::tempdir().unwrap();
        let configured = dir.path().to_string_lossy().into_owned();
        let s = Settings { working_dir: configured.clone(), ..Default::default() };
        assert_eq!(build_launch_spec("p", &s, "m").cwd.to_string_lossy(), configured);
    }
}
