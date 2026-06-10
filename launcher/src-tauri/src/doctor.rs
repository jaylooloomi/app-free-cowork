use crate::command::Runner;
use crate::http::Http;
use crate::version;
use std::path::PathBuf;
use std::time::Duration;

#[derive(Debug, Clone, PartialEq)]
pub enum Component { Ollama, OllamaUpgrade, ClaudeCode, Model }

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
    /// ensure_server poll: interval ms, attempts (production: 200ms × 50 = 10s; tests: 1ms × 1-3)
    pub serve_poll_ms: u64,
    pub serve_attempts: u32,
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
    match runner.run("ollama", &["--version"], Duration::from_secs(3)) {
        Err(_) => OllamaState::Missing,
        Ok(out) if !out.ok() => OllamaState::Missing,
        Ok(out) => match version::parse_ollama_version(&out.stdout) {
            Some(v) if version::meets_min(&v) => OllamaState::Ok,
            Some(_) => OllamaState::TooOld,
            // Unparseable-but-zero-exit: treat as TooOld — upgrade flow reinstalls latest, a sane recovery.
            None => OllamaState::TooOld,
        },
    }
}

fn server_alive(http: &dyn Http) -> bool {
    http.get(PING_URL, Duration::from_secs(1)).is_ok()
}

/// Start server if not alive; short-circuit on spawn failure; poll at poll_ms
/// intervals up to `attempts` times.
fn ensure_server(runner: &dyn Runner, http: &dyn Http, poll_ms: u64, attempts: u32) -> bool {
    if server_alive(http) { return true; }
    if runner.spawn_detached("ollama", &["serve"]).is_err() { return false; }
    for _ in 0..attempts {
        std::thread::sleep(Duration::from_millis(poll_ms));
        if server_alive(http) { return true; }
    }
    false
}

fn model_registered(runner: &dyn Runner, model: &str) -> bool {
    match runner.run("ollama", &["list"], Duration::from_secs(10)) {
        Ok(out) if out.ok() => out.stdout.lines().skip(1).filter_map(|l| l.split_whitespace().next()).any(|tok| {
            tok == model || (!model.contains(':') && tok.split(':').next() == Some(model))
        }),
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

    if !ensure_server(deps.runner, deps.http, deps.serve_poll_ms, deps.serve_attempts) {
        return Status::Degraded { reason: "Ollama 服務無法啟動".into() };
    }
    if !model_registered(deps.runner, model) {
        return Status::NeedsSetup { missing: vec![Component::Model] };
    }
    Status::Ready
}

/// Fast path: ping first, then check file presence. On healthy path (~2–6 ms)
/// NO subprocess is spawned. If ping fails or claude missing, falls back to
/// subprocess checks and, if needed, attempts to start the server.
pub fn quick_check(deps: &Deps) -> Status {
    if server_alive(deps.http) && claude_installed(&deps.claude_paths) {
        return Status::Ready; // healthy path: ~2-6ms, NO subprocess spawned
    }
    let ollama_present = matches!(ollama_state(deps.runner), OllamaState::Ok | OllamaState::TooOld);
    if !ollama_present || !claude_installed(&deps.claude_paths) {
        return Status::NeedsSetup { missing: vec![] };
    }
    if !ensure_server(deps.runner, deps.http, deps.serve_poll_ms, deps.serve_attempts) {
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

    const PING: &str = "http://127.0.0.1:11434/api/version";

    fn deps<'a>(r: &'a MockRunner, h: &'a MockHttp, claude: bool) -> Deps<'a> {
        deps_with_path(r, h, if claude {
            // use a real file that exists: the test binary's own crate manifest
            vec![PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml")]
        } else {
            vec![PathBuf::from("definitely/not/here.exe")]
        })
    }

    fn deps_with_path<'a>(r: &'a MockRunner, h: &'a MockHttp, claude_paths: Vec<PathBuf>) -> Deps<'a> {
        Deps { runner: r, http: h, claude_paths, serve_poll_ms: 1, serve_attempts: 2 }
    }

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
    fn dead_server_spawn_attempted_then_degraded() {
        let r = MockRunner::default()
            .on("ollama --version", 0, "ollama version is 0.30.6")
            .on("ollama list", 0, "NAME\nminimax-m2.7:cloud  -");
        let h = MockHttp::default(); // ping always fails
        let st = full_check(&deps(&r, &h, true), "minimax-m2.7:cloud");
        assert!(r.calls.lock().unwrap().iter().any(|c| c.starts_with("DETACHED ollama serve")));
        assert!(matches!(st, Status::Degraded { .. })); // can't start → Degraded
    }

    #[test]
    fn missing_local_model_reported_as_component() {
        let r = MockRunner::default()
            .on("ollama --version", 0, "ollama version is 0.30.6")
            .on("ollama list", 0, "NAME\nother:cloud  -");
        let h = MockHttp::default().on(PING, Ok("{}"));
        let result = full_check(&deps(&r, &h, true), "minimax-m2.7:cloud");
        assert_eq!(result, Status::NeedsSetup { missing: vec![Component::Model] });
        assert!(!r.calls.lock().unwrap().iter().any(|c| c.contains("pull")));
    }

    // --- model_registered tests ---

    #[test]
    fn model_registered_exact_tag_match() {
        let r = MockRunner::default()
            .on("ollama list", 0, "NAME\nminimax-m2.7:cloud  -  abc");
        assert!(model_registered(&r, "minimax-m2.7:cloud"));
    }

    #[test]
    fn model_registered_no_tag_matches_any_tag() {
        let r = MockRunner::default()
            .on("ollama list", 0, "NAME\nminimax-m2.7:cloud  -  abc");
        assert!(model_registered(&r, "minimax-m2.7"));
    }

    #[test]
    fn model_registered_no_prefix_collision() {
        let r = MockRunner::default()
            .on("ollama list", 0, "NAME\nminimax-m2.7:cloud  -  abc");
        assert!(!model_registered(&r, "minimax-m2"));
    }

    // --- ollama_state: garbage version output ---

    #[test]
    fn garbage_version_output_treated_as_too_old() {
        let r = MockRunner::default()
            .on("ollama --version", 0, "not a version string at all!!!!");
        assert!(matches!(ollama_state(&r), OllamaState::TooOld));
    }

    // --- quick_check tests ---

    #[test]
    fn quick_check_healthy_path_spawns_no_subprocess() {
        let r = MockRunner::default();
        let h = MockHttp::default().on(PING, Ok(r#"{"version":"0.30.6"}"#));
        let dir = tempfile::tempdir().unwrap();
        let claude_file = dir.path().join("claude.exe");
        std::fs::write(&claude_file, b"").unwrap();
        let d = deps_with_path(&r, &h, vec![claude_file]);
        assert_eq!(quick_check(&d), Status::Ready);
        assert!(r.calls.lock().unwrap().is_empty(), "no subprocess should be spawned on healthy path");
    }

    #[test]
    fn quick_check_cold_server_recovers_after_spawn() {
        let r = MockRunner::default()
            .on("ollama --version", 0, "ollama version is 0.30.6");
        // ping fails twice (quick_check initial check + ensure_server initial check),
        // then succeeds on the poll loop → spawn is recorded and server recovers
        let h = MockHttp::default()
            .on(PING, Ok(r#"{"version":"0.30.6"}"#))
            .failing_first(PING, 2);
        let dir = tempfile::tempdir().unwrap();
        let claude_file = dir.path().join("claude.exe");
        std::fs::write(&claude_file, b"").unwrap();
        let d = deps_with_path(&r, &h, vec![claude_file]);
        assert_eq!(quick_check(&d), Status::Ready);
        assert!(r.calls.lock().unwrap().iter().any(|c| c.starts_with("DETACHED ollama serve")));
    }

    #[test]
    fn quick_check_missing_everything_is_needs_setup() {
        let r = MockRunner::default(); // ollama missing
        let h = MockHttp::default();
        let d = deps_with_path(&r, &h, vec![PathBuf::from("definitely/not/here.exe")]);
        assert_eq!(quick_check(&d), Status::NeedsSetup { missing: vec![] });
    }
}
