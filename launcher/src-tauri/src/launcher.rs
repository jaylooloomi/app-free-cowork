use crate::settings::Settings;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq)]
pub struct LaunchSpec {
    pub program: String,
    pub args: Vec<String>,
    pub cwd: PathBuf,
    pub background: bool,
}

pub fn build_launch_spec(prompt: &str, s: &Settings, model: &str) -> LaunchSpec {
    let mut args: Vec<String> = vec![
        "launch".into(),
        "claude".into(),
        "--model".into(),
        model.into(),
        "--yes".into(),
        "--".into(),
    ];
    if s.background_mode {
        args.push("-p".into());
    }
    if s.cautious_mode {
        args.push("--permission-mode".into());
        args.push("acceptEdits".into());
    } else {
        args.push("--dangerously-skip-permissions".into());
    }
    args.push(prompt.to_string());
    LaunchSpec {
        program: "ollama".into(),
        args,
        cwd: s.effective_working_dir(),
        background: s.background_mode,
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

/// Spawn contract (v1.1):
/// - Returns the child PID (used by `task_stop` to `taskkill` background tasks).
/// - For BOTH modes the waiter thread ALWAYS calls `on_done(code, log_path, elapsed)`
///   when the process exits — task-queue chaining needs to observe every exit.
///   Notification policy (e.g. "foreground fast-fail", "stay silent when the user
///   closes a long-running console") lives in the caller (ipc), which decides
///   using `code`, `elapsed` and the launch mode.
/// - Foreground has no real log; log_path is passed through so callers can
///   construct messages uniformly (the file will be empty / non-existent).
pub fn spawn(spec: &LaunchSpec, log_path: PathBuf, on_done: Option<OnDone>) -> std::io::Result<u32> {
    use std::os::windows::process::CommandExt;
    use std::process::{Command, Stdio};
    let mut cmd = Command::new(&spec.program);
    cmd.args(&spec.args).current_dir(&spec.cwd);
    if spec.background {
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
        };
        let (tx, rx) = mpsc::channel();
        spawn(&spec, log.clone(), Some(Box::new(move |code, path, elapsed| {
            tx.send((code, path, elapsed)).unwrap();
        }))).unwrap();
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
        };
        let (tx, rx) = mpsc::channel();
        let pid = spawn(&spec, log, Some(Box::new(move |code, _path, elapsed| {
            tx.send((code, elapsed)).unwrap();
        }))).unwrap();
        assert!(pid > 0, "spawn must return the child PID");
        // v1.1 contract: waiter fires on EVERY exit (queue chaining), even foreground code 0
        let (code, elapsed) = rx.recv_timeout(std::time::Duration::from_secs(15)).unwrap();
        assert_eq!(code, 0);
        assert!(elapsed < std::time::Duration::from_secs(15));
    }

    #[test]
    fn foreground_default_args() {
        let s = Settings::default();
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
                "--dangerously-skip-permissions",
                "整理 \"桌面\" 並分類"
            ]
        );
        assert!(!spec.background);
    }

    #[test]
    fn cautious_mode_swaps_permission_flag() {
        let s = Settings { cautious_mode: true, ..Default::default() };
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
                "--permission-mode",
                "acceptEdits",
                "p"
            ]
        );
    }

    #[test]
    fn background_mode_adds_print_flag() {
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
                "-p",
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
        };
        let (tx, rx) = mpsc::channel();
        spawn(&spec, log.clone(), Some(Box::new(move |code, path, _elapsed| { tx.send((code, path)).unwrap(); }))).unwrap();
        let (code, path) = rx.recv_timeout(std::time::Duration::from_secs(15)).unwrap();
        assert_eq!(code, 3);
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("out-line"), "log missing stdout; got: {content}");
        assert!(content.contains("err-line"), "log missing stderr; got: {content}");
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
