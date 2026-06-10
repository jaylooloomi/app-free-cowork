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

/// Foreground: opens a new console window (Win11 routes to Windows Terminal by default).
/// Background: no window, stdout/stderr written to log file, on_done called when process exits.
pub fn spawn(
    spec: &LaunchSpec,
    log_path: PathBuf,
    on_done: Option<Box<dyn FnOnce(i32, PathBuf) + Send>>,
) -> std::io::Result<()> {
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
        let mut child = cmd.spawn()?;
        std::thread::spawn(move || {
            let code = child
                .wait()
                .map(|s| s.code().unwrap_or(-1))
                .unwrap_or(-1);
            if let Some(f) = on_done {
                f(code, log_path);
            }
        });
    } else {
        cmd.creation_flags(CREATE_NEW_CONSOLE).spawn()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::Settings;

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
    fn working_dir_defaults_to_home() {
        let spec = build_launch_spec("p", &Settings::default(), "m");
        assert_eq!(spec.cwd, dirs::home_dir().unwrap());
        let dir = tempfile::tempdir().unwrap();
        let configured = dir.path().to_string_lossy().into_owned();
        let s = Settings { working_dir: configured.clone(), ..Default::default() };
        assert_eq!(build_launch_spec("p", &s, "m").cwd.to_string_lossy(), configured);
    }
}
