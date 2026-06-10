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
        Ok(o) => StepResult {
            ok: false,
            detail: format!("{action} 失敗 (exit {}): {}", o.code, truncate(&o.stderr, 400)),
        },
        Err(e) => StepResult { ok: false, detail: format!("{action} 失敗: {e}") },
    }
}

fn truncate(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

pub const OLLAMA_SETUP_URL: &str = "https://ollama.com/download/OllamaSetup.exe";
const LONG: Duration = Duration::from_secs(1800);

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
        if let Ok(ref o) = r {
            if o.ok() {
                refresh_path();
                return result(r, "安裝 Ollama (winget)");
            }
        }
    }
    // fallback: direct download of the official installer (Inno Setup → /VERYSILENT)
    // Uses download_to_file to stream to disk — no get_bytes in http.rs
    let exe = temp_dir.join("OllamaSetup.exe");
    match http.download_to_file(OLLAMA_SETUP_URL, &exe, Duration::from_secs(600)) {
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
            refresh_path();
            result(r, "安裝 Ollama (直接下載)")
        }
    }
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

/// `ollama signin` opens a browser and waits for pairing; no short timeout (10 min max).
pub fn signin(runner: &dyn Runner) -> StepResult {
    result(
        runner.run("ollama", &["signin"], Duration::from_secs(600)),
        "登入 ollama.com",
    )
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
    let mut parts: Vec<PathBuf> = Vec::new();
    if let Some(home) = dirs::home_dir() {
        parts.push(home.join(".local").join("bin"));
    }
    if let Some(local) = dirs::data_local_dir() {
        parts.push(local.join("Programs").join("Ollama"));
    }
    let current = std::env::var("PATH").unwrap_or_default();
    let mut new_path = current.clone();
    for p in parts {
        let s = p.to_string_lossy().into_owned();
        if !current.to_lowercase().contains(&s.to_lowercase()) {
            new_path.push(';');
            new_path.push_str(&s);
        }
    }
    std::env::set_var("PATH", new_path);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::MockRunner;
    use crate::http::MockHttp;

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
    fn ollama_install_falls_back_to_direct_download_when_winget_missing() {
        let r = MockRunner::default(); // winget does not exist
        let h = MockHttp::default().on(OLLAMA_SETUP_URL, Ok("fake-installer-bytes"));
        let dir = tempfile::tempdir().unwrap();
        let res = install_ollama(&r, &h, dir.path().to_path_buf());
        // MockRunner has no entry for OllamaSetup.exe → run Err → res.ok == false
        // but the file must have been downloaded and the runner called
        assert!(!res.ok);
        assert!(dir.path().join("OllamaSetup.exe").exists());
        assert!(r
            .calls
            .lock()
            .unwrap()
            .iter()
            .any(|c| c.contains("OllamaSetup.exe /VERYSILENT")));
    }

    #[test]
    fn claude_install_uses_official_ps_one_liner() {
        let r = MockRunner::default().on("powershell -NoProfile", 0, "installed");
        assert!(install_claude(&r).ok);
        let calls = r.calls.lock().unwrap();
        assert!(calls[0].contains("irm https://claude.ai/install.ps1 | iex"));
    }

    #[test]
    fn register_model_pulls_stub() {
        let r = MockRunner::default().on("ollama pull", 0, "");
        assert!(register_model(&r, "minimax-m2.7:cloud").ok);
    }
}
