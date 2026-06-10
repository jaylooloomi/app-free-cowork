use crate::command::Runner;
use crate::http::Http;
use crate::version;
use std::path::PathBuf;
use std::time::Duration;

#[derive(Debug, Clone, PartialEq)]
pub enum Component { Ollama, OllamaUpgrade, ClaudeCode }

#[derive(Debug, Clone, PartialEq)]
pub enum Status {
    Ready,
    NeedsSetup { missing: Vec<Component> },
    Degraded { reason: String },
}

pub struct Deps<'a> {
    pub runner: &'a dyn Runner,
    pub http: &'a dyn Http,
    /// claude.exe candidate paths (production = PATH lookup + %USERPROFILE%\.local\bin\claude.exe; injected in tests)
    pub claude_paths: Vec<PathBuf>,
}

pub fn default_claude_paths() -> Vec<PathBuf> {
    let mut v = Vec::new();
    if let Some(home) = dirs::home_dir() {
        v.push(home.join(".local").join("bin").join("claude.exe"));
        v.push(home.join(".claude").join("local").join("claude.exe"));
    }
    if let Ok(path) = std::env::var("PATH") {
        for dir in std::env::split_paths(&path) { v.push(dir.join("claude.exe")); }
    }
    v
}

const PING_URL: &str = "http://127.0.0.1:11434/api/version";

fn claude_installed(paths: &[PathBuf]) -> bool { paths.iter().any(|p| p.exists()) }

enum OllamaState { Missing, TooOld, Ok }
fn ollama_state(runner: &dyn Runner) -> OllamaState {
    match runner.run("ollama", &["--version"], Duration::from_secs(10)) {
        Err(_) => OllamaState::Missing,
        Ok(out) if !out.ok() => OllamaState::Missing,
        Ok(out) => match version::parse_ollama_version(&out.stdout) {
            Some(v) if version::meets_min(&v) => OllamaState::Ok,
            Some(_) => OllamaState::TooOld,
            None => OllamaState::TooOld,
        },
    }
}

fn server_alive(http: &dyn Http) -> bool {
    http.get(PING_URL, Duration::from_secs(1)).is_ok()
}

/// Start server if not alive; poll at poll_ms intervals up to `attempts` times.
fn ensure_server(runner: &dyn Runner, http: &dyn Http, poll_ms: u64, attempts: u32) -> bool {
    if server_alive(http) { return true; }
    let _ = runner.spawn_detached("ollama", &["serve"]);
    for _ in 0..attempts {
        std::thread::sleep(Duration::from_millis(poll_ms));
        if server_alive(http) { return true; }
    }
    false
}

fn model_registered(runner: &dyn Runner, model: &str) -> bool {
    match runner.run("ollama", &["list"], Duration::from_secs(10)) {
        Ok(out) if out.ok() => out.stdout.lines().skip(1).any(|l| l.split_whitespace().next() == Some(model)),
        _ => false,
    }
}

pub fn full_check(deps: &Deps, model: &str) -> Status {
    let mut missing = Vec::new();
    match ollama_state(deps.runner) {
        OllamaState::Missing => missing.push(Component::Ollama),
        OllamaState::TooOld => missing.push(Component::OllamaUpgrade),
        OllamaState::Ok => {}
    }
    if !claude_installed(&deps.claude_paths) { missing.push(Component::ClaudeCode); }
    if !missing.is_empty() { return Status::NeedsSetup { missing }; }

    if !ensure_server(deps.runner, deps.http, 50, 3) {
        return Status::Degraded { reason: "Ollama 服務無法啟動".into() };
    }
    if !model_registered(deps.runner, model) {
        let pulled = deps.runner.run("ollama", &["pull", model], Duration::from_secs(120))
            .map(|o| o.ok()).unwrap_or(false);
        if !pulled { return Status::Degraded { reason: format!("無法註冊模型 {model}") }; }
    }
    Status::Ready
}

/// Fast path: only check file presence + ping (millisecond-level); detail errors
/// left for runtime error handling. Returns NeedsSetup{missing:vec![]} when
/// something is absent — caller should re-run full_check to get the list.
pub fn quick_check(deps: &Deps) -> Status {
    let ollama_present = matches!(ollama_state(deps.runner), OllamaState::Ok | OllamaState::TooOld);
    if !ollama_present || !claude_installed(&deps.claude_paths) {
        return Status::NeedsSetup { missing: vec![] };
    }
    if !ensure_server(deps.runner, deps.http, 200, 25) {
        return Status::Degraded { reason: "Ollama 服務無法啟動".into() };
    }
    Status::Ready
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::MockRunner;
    use crate::http::MockHttp;
    use std::path::PathBuf;

    fn deps<'a>(r: &'a MockRunner, h: &'a MockHttp, claude: bool) -> Deps<'a> {
        Deps {
            runner: r, http: h,
            claude_paths: if claude { vec![PathBuf::from("Cargo.toml")] } else { vec![PathBuf::from("definitely/not/here.exe")] },
        }
    }
    const PING: &str = "http://127.0.0.1:11434/api/version";

    #[test]
    fn ready_when_everything_present() {
        let r = MockRunner::default()
            .on("ollama --version", 0, "ollama version is 0.30.6")
            .on("ollama list", 0, "NAME\nminimax-m2.7:cloud  -  abc");
        let h = MockHttp::default().on(PING, Ok(r#"{"version":"0.30.6"}"#));
        assert_eq!(full_check(&deps(&r, &h, true), "minimax-m2.7:cloud"), Status::Ready);
    }
    #[test]
    fn needs_setup_lists_missing_components() {
        let r = MockRunner::default(); // ollama not present → run returns Err
        let h = MockHttp::default();
        match full_check(&deps(&r, &h, false), "m") {
            Status::NeedsSetup { missing } => {
                assert!(missing.contains(&Component::Ollama));
                assert!(missing.contains(&Component::ClaudeCode));
            }
            other => panic!("got {other:?}"),
        }
    }
    #[test]
    fn old_ollama_requires_upgrade() {
        let r = MockRunner::default().on("ollama --version", 0, "ollama version is 0.15.0");
        let h = MockHttp::default().on(PING, Ok("{}"));
        match full_check(&deps(&r, &h, true), "m") {
            Status::NeedsSetup { missing } => assert_eq!(missing, vec![Component::OllamaUpgrade]),
            other => panic!("got {other:?}"),
        }
    }
    #[test]
    fn dead_server_gets_started_then_ready() {
        let r = MockRunner::default()
            .on("ollama --version", 0, "ollama version is 0.30.6")
            .on("ollama list", 0, "NAME\nminimax-m2.7:cloud  -");
        let h = MockHttp::default(); // ping always fails
        let st = full_check(&deps(&r, &h, true), "minimax-m2.7:cloud");
        assert!(r.calls.lock().unwrap().iter().any(|c| c.starts_with("DETACHED ollama serve")));
        assert!(matches!(st, Status::Degraded { .. })); // can't start → Degraded
    }
    #[test]
    fn missing_local_model_triggers_pull() {
        let r = MockRunner::default()
            .on("ollama --version", 0, "ollama version is 0.30.6")
            .on("ollama list", 0, "NAME\nother:cloud  -")
            .on("ollama pull", 0, "");
        let h = MockHttp::default().on(PING, Ok("{}"));
        assert_eq!(full_check(&deps(&r, &h, true), "minimax-m2.7:cloud"), Status::Ready);
        assert!(r.calls.lock().unwrap().iter().any(|c| c.contains("pull minimax-m2.7:cloud")));
    }
}
