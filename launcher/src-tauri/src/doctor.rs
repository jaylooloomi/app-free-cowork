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
    /// quick_check version gate: set to `true` once `ollama --version` meets the minimum,
    /// so the healthy path spawns the subprocess at most once per cache lifetime
    /// (production: a process-wide `static`; tests: per-test instance for determinism).
    pub version_cache: &'a std::sync::OnceLock<bool>,
    /// ensure_server spawn cooldown: at most one `ollama serve` spawn per 30s window.
    /// Repeated spawns against a wedged port stack zombie processes and make a bad
    /// environment worse (observed during E2E on 2026-06-11). Production: process-wide
    /// `static`; tests: per-test instance.
    pub serve_spawn_gate: &'a std::sync::Mutex<Option<std::time::Instant>>,
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
/// The gate limits spawns to one per 30s window: a previous spawn may still be
/// warming up, and stacking `ollama serve` processes against a wedged port only
/// makes things worse. Within the cooldown we still poll (the earlier spawn may
/// come alive), we just don't spawn again.
/// `pub(crate)` so wizard steps that need a live daemon (signin/model) can reuse it.
pub(crate) fn ensure_server(
    runner: &dyn Runner,
    http: &dyn Http,
    poll_ms: u64,
    attempts: u32,
    gate: &std::sync::Mutex<Option<std::time::Instant>>,
) -> bool {
    if server_alive(http) { return true; }
    let should_spawn = {
        let mut g = gate.lock().unwrap();
        match *g {
            Some(t) if t.elapsed() < Duration::from_secs(30) => false,
            _ => {
                *g = Some(std::time::Instant::now());
                true
            }
        }
    };
    if should_spawn && runner.spawn_detached("ollama", &["serve"]).is_err() {
        return false;
    }
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

    if !ensure_server(deps.runner, deps.http, deps.serve_poll_ms, deps.serve_attempts, deps.serve_spawn_gate) {
        return Status::Degraded { reason: "Ollama 服務未回應，請重新啟動 Ollama 後再試".into() };
    }
    if !model_registered(deps.runner, model) {
        return Status::NeedsSetup { missing: vec![Component::Model] };
    }
    Status::Ready
}

/// Fast path: ping first, then check file presence. On healthy path the only
/// subprocess ever spawned is a single `ollama --version` (cached once OK), so
/// repeat calls are ~2–6 ms with NO subprocess. If ping fails or claude missing,
/// falls back to subprocess checks and, if needed, attempts to start the server.
pub fn quick_check(deps: &Deps) -> Status {
    if server_alive(deps.http) && claude_installed(&deps.claude_paths) {
        // Version gate: a running-but-too-old Ollama must not report Ready.
        // Only a positive result is cached — caching `false` would keep returning
        // NeedsSetup after the wizard upgrades Ollama mid-process.
        let version_ok = match deps.version_cache.get() {
            Some(ok) => *ok,
            None => {
                let ok = matches!(ollama_state(deps.runner), OllamaState::Ok);
                if ok {
                    let _ = deps.version_cache.set(true);
                }
                ok
            }
        };
        if version_ok {
            return Status::Ready; // healthy path: ~2-6ms once the version is cached
        }
        // Old (or unidentifiable) Ollama is serving → route to setup/upgrade.
        return Status::NeedsSetup { missing: vec![] };
    }
    let ollama_present = matches!(ollama_state(deps.runner), OllamaState::Ok | OllamaState::TooOld);
    if !ollama_present || !claude_installed(&deps.claude_paths) {
        return Status::NeedsSetup { missing: vec![] };
    }
    if !ensure_server(deps.runner, deps.http, deps.serve_poll_ms, deps.serve_attempts, deps.serve_spawn_gate) {
        return Status::Degraded { reason: "Ollama 服務未回應，請重新啟動 Ollama 後再試".into() };
    }
    Status::Ready
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::MockRunner;
    use crate::http::MockHttp;
    use std::path::PathBuf;
    use std::sync::OnceLock;

    const PING: &str = "http://127.0.0.1:11434/api/version";

    // Each test owns its OnceLock (NOT process-global) so the version gate stays
    // deterministic when tests share a process.
    fn deps<'a>(r: &'a MockRunner, h: &'a MockHttp, cache: &'a OnceLock<bool>, claude: bool) -> Deps<'a> {
        deps_with_path(r, h, cache, if claude {
            // use a real file that exists: the test binary's own crate manifest
            vec![PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml")]
        } else {
            vec![PathBuf::from("definitely/not/here.exe")]
        })
    }

    fn deps_with_path<'a>(
        r: &'a MockRunner,
        h: &'a MockHttp,
        cache: &'a OnceLock<bool>,
        claude_paths: Vec<PathBuf>,
    ) -> Deps<'a> {
        Deps {
            runner: r,
            http: h,
            claude_paths,
            serve_poll_ms: 1,
            serve_attempts: 2,
            version_cache: cache,
            // fresh gate per Deps (leaked: trivial in tests) → per-test cooldown isolation
            serve_spawn_gate: Box::leak(Box::new(std::sync::Mutex::new(None))),
        }
    }

    #[test]
    fn ready_when_everything_present() {
        let r = MockRunner::default()
            .on("ollama --version", 0, "ollama version is 0.30.6")
            .on("ollama list", 0, "NAME\nminimax-m2.7:cloud  -  abc");
        let h = MockHttp::default().on(PING, Ok(r#"{"version":"0.30.6"}"#));
        let cache = OnceLock::new();
        assert_eq!(full_check(&deps(&r, &h, &cache, true), "minimax-m2.7:cloud"), Status::Ready);
    }

    #[test]
    fn needs_setup_lists_missing_components() {
        let r = MockRunner::default(); // ollama not present → run returns Err
        let h = MockHttp::default();
        let cache = OnceLock::new();
        match full_check(&deps(&r, &h, &cache, false), "m") {
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
        let cache = OnceLock::new();
        match full_check(&deps(&r, &h, &cache, true), "m") {
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
        let cache = OnceLock::new();
        let st = full_check(&deps(&r, &h, &cache, true), "minimax-m2.7:cloud");
        assert!(r.calls.lock().unwrap().iter().any(|c| c.starts_with("DETACHED ollama serve")));
        assert!(matches!(st, Status::Degraded { .. })); // can't start → Degraded
    }

    #[test]
    fn missing_local_model_reported_as_component() {
        let r = MockRunner::default()
            .on("ollama --version", 0, "ollama version is 0.30.6")
            .on("ollama list", 0, "NAME\nother:cloud  -");
        let h = MockHttp::default().on(PING, Ok("{}"));
        let cache = OnceLock::new();
        let result = full_check(&deps(&r, &h, &cache, true), "minimax-m2.7:cloud");
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
    fn quick_check_healthy_path_runs_version_gate_once_then_caches() {
        let r = MockRunner::default().on("ollama --version", 0, "ollama version is 0.30.6");
        let h = MockHttp::default().on(PING, Ok(r#"{"version":"0.30.6"}"#));
        let dir = tempfile::tempdir().unwrap();
        let claude_file = dir.path().join("claude.exe");
        std::fs::write(&claude_file, b"").unwrap();
        let cache = OnceLock::new();
        let d = deps_with_path(&r, &h, &cache, vec![claude_file]);
        // First healthy call: exactly one subprocess — the version gate.
        assert_eq!(quick_check(&d), Status::Ready);
        {
            let calls = r.calls.lock().unwrap();
            assert_eq!(calls.len(), 1, "first healthy call spawns at most one subprocess; got {calls:?}");
            assert!(calls[0].starts_with("ollama --version"), "the single call must be the version gate; got {calls:?}");
        }
        // Second call: version is cached → NO new subprocess.
        assert_eq!(quick_check(&d), Status::Ready);
        assert_eq!(r.calls.lock().unwrap().len(), 1, "cached version gate must not spawn again");
    }

    #[test]
    fn quick_check_healthy_ping_but_old_ollama_is_needs_setup() {
        // Server answers the ping but the binary is below the minimum version →
        // must NOT report Ready; route to wizard (full_check will plan the upgrade).
        let r = MockRunner::default().on("ollama --version", 0, "ollama version is 0.15.0");
        let h = MockHttp::default().on(PING, Ok("{}"));
        let dir = tempfile::tempdir().unwrap();
        let claude_file = dir.path().join("claude.exe");
        std::fs::write(&claude_file, b"").unwrap();
        let cache = OnceLock::new();
        let d = deps_with_path(&r, &h, &cache, vec![claude_file]);
        assert_eq!(quick_check(&d), Status::NeedsSetup { missing: vec![] });
        // A negative result must NOT be cached: an upgrade mid-process should recover.
        assert!(cache.get().is_none(), "version gate must not cache a negative result");
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
        let cache = OnceLock::new();
        let d = deps_with_path(&r, &h, &cache, vec![claude_file]);
        assert_eq!(quick_check(&d), Status::Ready);
        assert!(r.calls.lock().unwrap().iter().any(|c| c.starts_with("DETACHED ollama serve")));
    }

    #[test]
    fn quick_check_missing_everything_is_needs_setup() {
        let r = MockRunner::default(); // ollama missing
        let h = MockHttp::default();
        let cache = OnceLock::new();
        let d = deps_with_path(&r, &h, &cache, vec![PathBuf::from("definitely/not/here.exe")]);
        assert_eq!(quick_check(&d), Status::NeedsSetup { missing: vec![] });
    }

    // --- ensure_server (pub(crate): reused by wizard signin/model steps) ---

    #[test]
    fn ensure_server_alive_returns_true_without_spawning() {
        let r = MockRunner::default();
        let h = MockHttp::default().on(PING, Ok("{}"));
        assert!(ensure_server(&r, &h, 1, 2, &std::sync::Mutex::new(None)));
        assert!(r.calls.lock().unwrap().is_empty(), "alive server must not trigger a spawn");
    }

    #[test]
    fn ensure_server_spawns_and_polls_until_alive() {
        let r = MockRunner::default();
        let h = MockHttp::default().on(PING, Ok("{}")).failing_first(PING, 1);
        assert!(ensure_server(&r, &h, 1, 2, &std::sync::Mutex::new(None)));
        assert!(r.calls.lock().unwrap().iter().any(|c| c.starts_with("DETACHED ollama serve")));
    }

    #[test]
    fn ensure_server_returns_false_when_server_never_answers() {
        let r = MockRunner::default();
        let h = MockHttp::default(); // ping always fails
        assert!(!ensure_server(&r, &h, 1, 2, &std::sync::Mutex::new(None)));
        assert!(r.calls.lock().unwrap().iter().any(|c| c.starts_with("DETACHED ollama serve")));
    }

    #[test]
    fn ensure_server_spawns_at_most_once_within_cooldown() {
        let r = MockRunner::default();
        let h = MockHttp::default(); // ping always fails
        let gate = std::sync::Mutex::new(None);
        assert!(!ensure_server(&r, &h, 1, 1, &gate));
        assert!(!ensure_server(&r, &h, 1, 1, &gate)); // within 30s window
        let spawns = r.calls.lock().unwrap().iter().filter(|c| c.starts_with("DETACHED")).count();
        assert_eq!(spawns, 1, "second call within cooldown must not spawn another serve");
    }
}
