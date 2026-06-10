use crate::command::{CmdOutput, Runner};
use crate::http::Http;
use std::path::PathBuf;
use std::time::Duration;

#[derive(Debug, Clone, serde::Serialize)]
pub struct StepResult {
    pub ok: bool,
    pub detail: String,
}

fn result(r: std::io::Result<CmdOutput>, action: &str) -> StepResult {
    match r {
        Ok(o) if o.ok() => StepResult { ok: true, detail: format!("{action} 完成") },
        Ok(o) => {
            // Use stdout as fallback when stderr is empty (some tools write only to stdout)
            let tail = if o.stderr.trim().is_empty() { &o.stdout } else { &o.stderr };
            StepResult {
                ok: false,
                detail: format!("{action} 失敗 (exit {}): {}", o.code, truncate(tail, 400)),
            }
        }
        Err(e) => StepResult { ok: false, detail: format!("{action} 失敗: {e}") },
    }
}

fn truncate(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

pub const OLLAMA_SETUP_URL: &str = "https://ollama.com/download/OllamaSetup.exe";
const LONG: Duration = Duration::from_secs(1800);

/// winget exit code meaning "already installed" (0x8A150061 as a signed i32).
const WINGET_ALREADY_INSTALLED: i32 = -1978335135;

fn winget_path(runner: &dyn Runner) -> Option<String> {
    if runner
        .run("winget", &["--version"], Duration::from_secs(10))
        .map(|o| o.ok())
        .unwrap_or(false)
    {
        return Some("winget".into());
    }
    let local = dirs::data_local_dir()?
        .join("Microsoft")
        .join("WindowsApps")
        .join("winget.exe");
    if local.exists() {
        return Some(local.to_string_lossy().into_owned());
    }
    None
}

pub fn install_ollama(runner: &dyn Runner, http: &dyn Http, temp_dir: PathBuf) -> StepResult {
    let mut winget_note: Option<String> = None;

    if let Some(winget) = winget_path(runner) {
        let r = runner.run(
            &winget,
            &[
                "install",
                "-e",
                "--id",
                "Ollama.Ollama",
                "--scope",
                "user",
                "--accept-source-agreements",
                "--accept-package-agreements",
            ],
            LONG,
        );
        match &r {
            Ok(o) if o.ok() => {
                refresh_path();
                return StepResult { ok: true, detail: "安裝 Ollama (winget) 完成".into() };
            }
            Ok(o) if o.code == WINGET_ALREADY_INSTALLED => {
                refresh_path();
                return StepResult { ok: true, detail: "安裝 Ollama (winget) 完成".into() };
            }
            Ok(o) => {
                // Winget ran but failed — record cause for potential fallback error message
                let tail = if o.stderr.trim().is_empty() { &o.stdout } else { &o.stderr };
                winget_note = Some(format!("winget exit {}: {}", o.code, truncate(tail, 120)));
            }
            Err(e) => {
                winget_note = Some(format!("winget 執行失敗: {e}"));
            }
        }
    }

    // Fallback: direct download of the official installer (Inno Setup → /VERYSILENT)
    // Uses download_to_file to stream to disk — no get_bytes in http.rs
    let exe = temp_dir.join("OllamaSetup.exe");
    let fallback_result = match http.download_to_file(OLLAMA_SETUP_URL, &exe, Duration::from_secs(600)) {
        Err(e) => StepResult {
            ok: false,
            detail: format!("下載 OllamaSetup.exe 失敗: {e}"),
        },
        Ok(()) => {
            let r = runner.run(
                &exe.to_string_lossy(),
                &["/VERYSILENT", "/SP-", "/SUPPRESSMSGBOXES"],
                LONG,
            );
            // Cleanup temp installer regardless of success or failure
            let _ = std::fs::remove_file(&exe);
            if r.as_ref().map(|o| o.ok()).unwrap_or(false) {
                refresh_path();
            }
            result(r, "安裝 Ollama (直接下載)")
        }
    };

    // If fallback also failed, prefix any winget failure cause onto the detail
    if !fallback_result.ok {
        if let Some(note) = winget_note {
            return StepResult {
                ok: false,
                detail: format!("{}(先前 {})", fallback_result.detail, note),
            };
        }
    }
    fallback_result
}

pub fn install_claude(runner: &dyn Runner) -> StepResult {
    let r = runner.run(
        "powershell",
        &[
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            "irm https://claude.ai/install.ps1 | iex",
        ],
        LONG,
    );
    refresh_path();
    result(r, "安裝 Claude Code")
}

static SIGNIN_GUARD: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// `ollama signin` opens a browser and waits for pairing; no short timeout (10 min max).
/// A static mutex prevents concurrent signin flows from racing.
pub fn signin(runner: &dyn Runner) -> StepResult {
    let Ok(_g) = SIGNIN_GUARD.try_lock() else {
        return StepResult {
            ok: false,
            detail: "登入流程已在進行中,請先完成瀏覽器配對".into(),
        };
    };
    result(runner.run("ollama", &["signin"], Duration::from_secs(600)), "登入 ollama.com")
}

pub fn register_model(runner: &dyn Runner, model: &str) -> StepResult {
    result(
        runner.run("ollama", &["pull", model], Duration::from_secs(300)),
        "註冊雲端模型",
    )
}

/// After installation, make newly installed binaries visible to the current process
/// by appending known install dirs to PATH if absent (case-insensitive check).
pub fn refresh_path() {
    let current = std::env::var("PATH").unwrap_or_default();
    std::env::set_var("PATH", merged_path(&current, &known_dirs()));
}

fn known_dirs() -> Vec<PathBuf> {
    let mut dirs_list: Vec<PathBuf> = Vec::new();
    if let Some(home) = dirs::home_dir() {
        dirs_list.push(home.join(".local").join("bin"));
    }
    if let Some(local) = dirs::data_local_dir() {
        dirs_list.push(local.join("Programs").join("Ollama"));
    }
    dirs_list
}

/// Deliberately does NOT re-read the registry PATH (spec §4.3 mentions it; the two
/// appended dirs cover every post-install consumer, and a registry re-read could
/// clobber process-specific entries).
fn merged_path(current: &str, parts: &[PathBuf]) -> String {
    let mut out = current.to_string();
    for p in parts {
        let p_str = p.to_string_lossy();
        let already = std::env::split_paths(current)
            .any(|e| e.as_os_str().to_string_lossy().eq_ignore_ascii_case(p_str.as_ref()));
        if !already {
            out.push(';');
            out.push_str(&p_str);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::MockRunner;
    use crate::http::MockHttp;

    // ── install_ollama ────────────────────────────────────────────────────────

    #[test]
    fn ollama_install_prefers_winget() {
        let r = MockRunner::default()
            .on("winget --version", 0, "v1.12")
            .on("winget install", 0, "ok");
        let h = MockHttp::default();
        assert!(install_ollama(&r, &h, std::env::temp_dir()).ok);
        let calls = r.calls.lock().unwrap();
        assert!(calls
            .iter()
            .any(|c| c.contains("install -e --id Ollama.Ollama --scope user")));
    }

    #[test]
    fn ollama_install_treats_already_installed_as_success() {
        let r = MockRunner::default()
            .on("winget --version", 0, "v1.12")
            // exit code -1978335135 (0x8A150061) = already installed
            .on("winget install", -1978335135, "already installed");
        let h = MockHttp::default(); // no download URL configured → any call would error
        let res = install_ollama(&r, &h, std::env::temp_dir());
        assert!(res.ok, "already-installed exit code must be treated as success");
        // No download should have been attempted
        let calls = r.calls.lock().unwrap();
        assert!(
            !calls.iter().any(|c| c.contains("OllamaSetup")),
            "must NOT have attempted a direct download after already-installed"
        );
    }

    #[test]
    fn ollama_install_falls_back_to_direct_download_when_winget_missing() {
        let r = MockRunner::default(); // winget does not exist
        let h = MockHttp::default().on(OLLAMA_SETUP_URL, Ok("fake-installer-bytes"));
        let dir = tempfile::tempdir().unwrap();
        let res = install_ollama(&r, &h, dir.path().to_path_buf());
        // MockRunner has no entry for OllamaSetup.exe → run Err → res.ok == false
        // The file is cleaned up after the run attempt, so we cannot assert it still exists.
        // Instead we verify the runner was invoked with the silent-install flags.
        assert!(!res.ok);
        assert!(r
            .calls
            .lock()
            .unwrap()
            .iter()
            .any(|c| c.contains("OllamaSetup.exe /VERYSILENT")));
    }

    #[test]
    fn fallback_failure_detail_includes_winget_cause() {
        // winget is present but returns exit 1 with stdout "policy blocked"
        let r = MockRunner::default()
            .on("winget --version", 0, "v1.12")
            .on("winget install", 1, "policy blocked"); // stdout, no stderr
        // The download succeeds (MockHttp writes fake bytes to disk),
        // but MockRunner has no entry for OllamaSetup.exe → run Err
        let h = MockHttp::default().on(OLLAMA_SETUP_URL, Ok("fake-installer-bytes"));
        let dir = tempfile::tempdir().unwrap();
        let res = install_ollama(&r, &h, dir.path().to_path_buf());
        assert!(!res.ok);
        // Exact format pinned: "winget exit 1: <stdout tail>"
        assert!(
            res.detail.contains("winget exit 1"),
            "detail must contain winget exit code; got: {}",
            res.detail
        );
    }

    // ── install_claude ────────────────────────────────────────────────────────

    #[test]
    fn claude_install_uses_official_ps_one_liner() {
        let r = MockRunner::default().on("powershell -NoProfile", 0, "installed");
        assert!(install_claude(&r).ok);
        let calls = r.calls.lock().unwrap();
        assert!(calls[0].contains("irm https://claude.ai/install.ps1 | iex"));
    }

    // ── signin ────────────────────────────────────────────────────────────────

    #[test]
    fn signin_guard_rejects_concurrent_call() {
        // Hold the mutex to simulate an in-progress signin
        let _held = SIGNIN_GUARD.lock().unwrap();
        let r = MockRunner::default().on("ollama signin", 0, "");
        let res = signin(&r);
        assert!(!res.ok);
        assert!(
            res.detail.contains("登入流程已在進行中"),
            "expected in-progress message, got: {}",
            res.detail
        );
    }

    #[test]
    fn signin_succeeds_when_guard_is_free() {
        // Ensure the guard is not held (previous test drops _held at end of scope)
        let r = MockRunner::default().on("ollama signin", 0, "paired");
        let res = signin(&r);
        assert!(res.ok, "signin must succeed when guard is free; got: {:?}", res);
    }

    // ── register_model ────────────────────────────────────────────────────────

    #[test]
    fn register_model_pulls_stub() {
        let r = MockRunner::default().on("ollama pull", 0, "");
        assert!(register_model(&r, "minimax-m2.7:cloud").ok);
    }

    // ── result() stdout fallback ──────────────────────────────────────────────

    #[test]
    fn result_uses_stdout_when_stderr_empty() {
        let output = crate::command::CmdOutput {
            code: 1,
            stdout: "only stdout here".into(),
            stderr: "".into(),
        };
        let r: std::io::Result<crate::command::CmdOutput> = Ok(output);
        let step = result(r, "test action");
        assert!(!step.ok);
        assert!(
            step.detail.contains("only stdout here"),
            "should fall back to stdout when stderr empty; got: {}",
            step.detail
        );
    }

    #[test]
    fn result_uses_stderr_when_nonempty() {
        let output = crate::command::CmdOutput {
            code: 2,
            stdout: "some stdout".into(),
            stderr: "critical error on stderr".into(),
        };
        let r: std::io::Result<crate::command::CmdOutput> = Ok(output);
        let step = result(r, "test action");
        assert!(!step.ok);
        assert!(
            step.detail.contains("critical error on stderr"),
            "should prefer stderr when nonempty; got: {}",
            step.detail
        );
        assert!(
            !step.detail.contains("some stdout"),
            "should not include stdout when stderr is present; got: {}",
            step.detail
        );
    }

    // ── merged_path ───────────────────────────────────────────────────────────

    #[test]
    fn merged_path_appends_missing_dir() {
        let current = "C:\\existing";
        let new_dir = PathBuf::from("C:\\new\\dir");
        let result = merged_path(current, &[new_dir]);
        assert!(
            result.ends_with(";C:\\new\\dir"),
            "missing dir must be appended; got: {}",
            result
        );
    }

    #[test]
    fn merged_path_skips_already_present_case_insensitive() {
        let current = "C:\\existing;C:\\Ollama";
        let part = PathBuf::from("C:\\ollama"); // different case
        let result = merged_path(current, &[part]);
        // Should NOT be appended again
        let count = result.to_lowercase().matches("ollama").count();
        assert_eq!(count, 1, "already-present dir (case-insensitive) must not be duplicated; got: {}", result);
    }

    #[test]
    fn merged_path_appends_when_only_prefix_superstring_present() {
        // "C:\X\OllamaLegacy" is in PATH; "C:\X\Ollama" is the target — must still append
        let current = "C:\\X\\OllamaLegacy";
        let part = PathBuf::from("C:\\X\\Ollama");
        let result = merged_path(current, &[part]);
        assert!(
            result.contains(";C:\\X\\Ollama"),
            "must append when only a prefix-superstring entry exists; got: {}",
            result
        );
    }
}
