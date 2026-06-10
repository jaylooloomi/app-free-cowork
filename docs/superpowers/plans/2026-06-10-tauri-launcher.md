# Free Claude Code 全域快捷鍵啟動器 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 一個 Tauri v2 系統匣常駐 app:按 `Alt+H` 喚出輸入框,輸入自然語言後自動補齊環境(Ollama + Claude Code)並以 `ollama launch claude` 執行需求。

**Architecture:** Rust 核心(設定/環境醫生/安裝引擎/啟動器,全部純邏輯 + trait 注入可測)+ Svelte WebView UI(輸入面板/安裝精靈/設定頁,依視窗 label 路由)。所有子程序以 arg 陣列 spawn,不經 shell。規格:`docs/superpowers/specs/2026-06-10-tauri-hotkey-launcher-design.md`(實作時必須先讀)。

**Tech Stack:** Tauri 2.x、Rust(serde、semver、ureq、wait-timeout、chrono、tempfile[dev])、Svelte + TypeScript + Vite、外掛:global-shortcut / autostart / single-instance / notification / updater。

**Conventions(每個任務都適用):**
- 工作目錄:repo 根 = `D:\git\free-claude-code\free-claude-code`;Tauri 專案在 `launcher/`;Rust 測試在 `launcher/src-tauri` 下跑 `cargo test`。
- TDD:先寫測試 → 跑出 FAIL → 最小實作 → PASS → commit。commit 訊息英文、現在式,結尾加 `Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>`。
- 不 push、不動 main。
- 外掛 API 以官方文件為準:若計畫中的外掛呼叫簽名與當前版本不符,以編譯器與 https://v2.tauri.app 文件為準修正,核心邏輯不得改變。
- UI 字串全部進 `launcher/src/lib/strings.ts`,繁體中文。

---

## File Structure

```
launcher/
├── package.json / vite.config.ts / svelte.config.js / tsconfig.json / index.html
├── src/                          # frontend
│   ├── main.ts                   # mount App
│   ├── App.svelte                # 依視窗 label 路由 palette/wizard/settings
│   └── lib/
│       ├── api.ts                # typed invoke 包裝
│       ├── strings.ts            # zh-TW 字串集中
│       ├── Palette.svelte
│       ├── Wizard.svelte
│       └── Settings.svelte
└── src-tauri/
    ├── Cargo.toml / tauri.conf.json / capabilities/default.json / icons/
    └── src/
        ├── main.rs               # entry(呼叫 lib.rs run)
        ├── lib.rs                # app 組裝:plugins、tray、windows、shortcut、argv
        ├── settings.rs           # Settings struct + load/save + history
        ├── version.rs            # ollama 版本解析與最低版本判斷
        ├── command.rs            # Runner trait + SystemRunner + MockRunner
        ├── http.rs               # Http trait + UreqHttp
        ├── catalog.rs            # 雲端模型目錄解析 + fallback 選擇
        ├── doctor.rs             # 環境體檢狀態機
        ├── logging.rs            # run log 建立與輪替
        ├── launcher.rs           # 指令組裝 + 前景/背景 spawn
        ├── bootstrap.rs          # 精靈安裝步驟
        └── ipc.rs                # #[tauri::command] 集合
```

模組相依方向:`ipc → {doctor, bootstrap, launcher, settings, catalog} → {command, http, version, logging}`。`lib.rs` 只做組裝。

---

### Task 1: Scaffold Tauri 專案

**Files:** Create: `launcher/`(整個 scaffold)

- [ ] **Step 1: 確認 Rust 工具鏈可用**

Run: `cargo --version && rustc --version`
Expected: 1.9x 版本字串。若 link.exe 缺失(MSVC 未裝完)→ 等待 VS Build Tools 安裝完成再繼續。

- [ ] **Step 2: 產生 scaffold**

在 repo 根執行(非互動):
```bash
npm create tauri-app@latest launcher -- --manager npm --template svelte-ts --yes
cd launcher && npm install
```

- [ ] **Step 3: 首次編譯驗證**

Run: `cd launcher/src-tauri && cargo check`
Expected: 成功(首次需數分鐘下載 crates)。

- [ ] **Step 4: 加入 Rust 依賴**

`launcher/src-tauri/Cargo.toml` `[dependencies]` 加入(版本以 `cargo add` 當下解析為準):
```bash
cd launcher/src-tauri
cargo add serde --features derive
cargo add serde_json semver ureq chrono wait-timeout dirs
cargo add tauri-plugin-global-shortcut tauri-plugin-autostart tauri-plugin-single-instance tauri-plugin-notification
cargo add --dev tempfile
cargo check
```
Expected: 成功。

- [ ] **Step 5: Commit**

```bash
git add launcher && git commit -m "Scaffold Tauri v2 launcher project with svelte-ts template"
```

---

### Task 2: version.rs + command.rs + http.rs(基礎層)

**Files:**
- Create: `launcher/src-tauri/src/version.rs`、`launcher/src-tauri/src/command.rs`、`launcher/src-tauri/src/http.rs`
- Modify: `launcher/src-tauri/src/lib.rs`(加 `pub mod`)

- [ ] **Step 1: 寫 version.rs 失敗測試**

```rust
// version.rs 尾端
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_ollama_version_line() {
        assert_eq!(parse_ollama_version("ollama version is 0.30.6").unwrap().to_string(), "0.30.6");
        assert_eq!(parse_ollama_version("ollama version is 0.15.6\nWarning: foo").unwrap().to_string(), "0.15.6");
        assert!(parse_ollama_version("garbage").is_none());
    }
    #[test]
    fn min_version_gate() {
        assert!(meets_min(&parse_ollama_version("ollama version is 0.15.6").unwrap()));
        assert!(meets_min(&parse_ollama_version("ollama version is 0.30.0").unwrap()));
        assert!(!meets_min(&parse_ollama_version("ollama version is 0.15.5").unwrap()));
        assert!(!meets_min(&parse_ollama_version("ollama version is 0.14.9").unwrap()));
    }
}
```

- [ ] **Step 2: 跑測試確認失敗**(`cargo test version::` → compile error)

- [ ] **Step 3: 實作 version.rs**

```rust
use semver::Version;

pub fn min_ollama() -> Version { Version::new(0, 15, 6) }

pub fn parse_ollama_version(stdout: &str) -> Option<Version> {
    stdout.lines().find_map(|line| {
        let token = line.trim().strip_prefix("ollama version is ")?.trim();
        Version::parse(token.split_whitespace().next()?).ok()
    })
}

pub fn meets_min(v: &Version) -> bool { *v >= min_ollama() }
```

- [ ] **Step 4: 實作 command.rs(Runner trait;測試用 MockRunner 同檔提供)**

```rust
use std::io;
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct CmdOutput { pub code: i32, pub stdout: String, pub stderr: String }
impl CmdOutput { pub fn ok(&self) -> bool { self.code == 0 } }

pub trait Runner: Send + Sync {
    /// 同步執行並收集輸出;逾時 kill 並回 io::Error。
    fn run(&self, program: &str, args: &[&str], timeout: Duration) -> io::Result<CmdOutput>;
    /// 發射後不管(隱藏視窗),用於 `ollama serve`。
    fn spawn_detached(&self, program: &str, args: &[&str]) -> io::Result<()>;
}

pub struct SystemRunner;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

impl Runner for SystemRunner {
    fn run(&self, program: &str, args: &[&str], timeout: Duration) -> io::Result<CmdOutput> {
        use std::os::windows::process::CommandExt;
        use std::process::{Command, Stdio};
        use wait_timeout::ChildExt;
        let mut child = Command::new(program)
            .args(args)
            .creation_flags(CREATE_NO_WINDOW)
            .stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped())
            .spawn()?;
        let status = match child.wait_timeout(timeout)? {
            Some(s) => s,
            None => { let _ = child.kill(); return Err(io::Error::new(io::ErrorKind::TimedOut, format!("{program} timed out"))); }
        };
        let mut out = String::new(); let mut err = String::new();
        use std::io::Read;
        if let Some(mut s) = child.stdout.take() { let _ = s.read_to_string(&mut out); }
        if let Some(mut s) = child.stderr.take() { let _ = s.read_to_string(&mut err); }
        Ok(CmdOutput { code: status.code().unwrap_or(-1), stdout: out, stderr: err })
    }
    fn spawn_detached(&self, program: &str, args: &[&str]) -> io::Result<()> {
        use std::os::windows::process::CommandExt;
        use std::process::{Command, Stdio};
        Command::new(program).args(args).creation_flags(CREATE_NO_WINDOW)
            .stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null())
            .spawn().map(|_| ())
    }
}

/// 測試替身:以 (program, first_arg) 對應預設輸出。
#[derive(Default)]
pub struct MockRunner {
    pub responses: std::collections::HashMap<String, CmdOutput>,
    pub calls: std::sync::Mutex<Vec<String>>,
}
impl MockRunner {
    pub fn on(mut self, key: &str, code: i32, stdout: &str) -> Self {
        self.responses.insert(key.into(), CmdOutput { code, stdout: stdout.into(), stderr: String::new() });
        self
    }
    fn key(program: &str, args: &[&str]) -> String {
        if args.is_empty() { program.to_string() } else { format!("{program} {}", args[0]) }
    }
}
impl Runner for MockRunner {
    fn run(&self, program: &str, args: &[&str], _t: Duration) -> io::Result<CmdOutput> {
        let key = Self::key(program, args);
        self.calls.lock().unwrap().push(format!("{program} {}", args.join(" ")));
        self.responses.get(&key).cloned()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, key))
    }
    fn spawn_detached(&self, program: &str, args: &[&str]) -> io::Result<()> {
        self.calls.lock().unwrap().push(format!("DETACHED {program} {}", args.join(" ")));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mock_runner_returns_configured_output_and_records_calls() {
        let r = MockRunner::default().on("ollama --version", 0, "ollama version is 0.30.6");
        let out = r.run("ollama", &["--version"], Duration::from_secs(5)).unwrap();
        assert!(out.ok());
        assert_eq!(out.stdout, "ollama version is 0.30.6");
        assert!(r.run("missing", &[], Duration::from_secs(1)).is_err());
        assert_eq!(r.calls.lock().unwrap().len(), 2);
    }
    #[test]
    fn system_runner_executes_real_command() {
        let out = SystemRunner.run("cmd", &["/c", "echo hi"], Duration::from_secs(10)).unwrap();
        assert!(out.ok());
        assert!(out.stdout.contains("hi"));
    }
}
```

- [ ] **Step 5: 實作 http.rs**

```rust
use std::time::Duration;

pub trait Http: Send + Sync {
    fn get(&self, url: &str, timeout: Duration) -> Result<String, String>;
    fn get_bytes(&self, url: &str, timeout: Duration) -> Result<Vec<u8>, String>;
}

pub struct UreqHttp;
impl Http for UreqHttp {
    fn get(&self, url: &str, timeout: Duration) -> Result<String, String> {
        let agent = ureq::AgentBuilder::new().timeout(timeout).build();
        agent.get(url).call().map_err(|e| e.to_string())?
            .into_string().map_err(|e| e.to_string())
    }
    fn get_bytes(&self, url: &str, timeout: Duration) -> Result<Vec<u8>, String> {
        let agent = ureq::AgentBuilder::new().timeout(timeout).build();
        let resp = agent.get(url).call().map_err(|e| e.to_string())?;
        let mut buf = Vec::new();
        use std::io::Read;
        resp.into_reader().read_to_end(&mut buf).map_err(|e| e.to_string())?;
        Ok(buf)
    }
}

/// 測試替身:url → 回應。
#[derive(Default)]
pub struct MockHttp { pub responses: std::collections::HashMap<String, Result<String, String>> }
impl MockHttp {
    pub fn on(mut self, url: &str, resp: Result<&str, &str>) -> Self {
        self.responses.insert(url.into(), resp.map(String::from).map_err(String::from));
        self
    }
}
impl Http for MockHttp {
    fn get(&self, url: &str, _t: Duration) -> Result<String, String> {
        self.responses.get(url).cloned().unwrap_or(Err(format!("unmocked {url}")))
    }
    fn get_bytes(&self, url: &str, t: Duration) -> Result<Vec<u8>, String> {
        self.get(url, t).map(|s| s.into_bytes())
    }
}
```
(ureq 3.x 的 API 若為 `ureq::Agent::config_builder()` 等新形態,依編譯器修正,行為不變。)

- [ ] **Step 6: lib.rs 加模組宣告**(`pub mod version; pub mod command; pub mod http;`)

- [ ] **Step 7: 跑測試**:`cargo test` → 全 PASS。

- [ ] **Step 8: Commit** `feat: add version parsing, command runner and http abstractions`

---

### Task 3: settings.rs

**Files:** Create: `launcher/src-tauri/src/settings.rs`;Modify: `lib.rs`(加 `pub mod settings;`)

- [ ] **Step 1: 失敗測試**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn defaults_are_per_spec() {
        let s = Settings::default();
        assert_eq!(s.hotkey, "Alt+H");
        assert_eq!(s.model, "minimax-m2.7:cloud");
        assert!(!s.cautious_mode);
        assert!(!s.background_mode);
        assert_eq!(s.working_dir, "");
        assert!(s.autostart);
        assert!(s.history.is_empty());
        assert_eq!(s.signin_state, SigninState::Unknown);
    }
    #[test]
    fn load_missing_or_corrupt_returns_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("settings.json");
        assert_eq!(load(&p), Settings::default());
        std::fs::write(&p, "{not json").unwrap();
        assert_eq!(load(&p), Settings::default());
    }
    #[test]
    fn save_then_load_roundtrip_and_partial_json_keeps_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("settings.json");
        let mut s = Settings::default();
        s.model = "qwen3-coder-next:cloud".into();
        save(&p, &s).unwrap();
        assert_eq!(load(&p), s);
        std::fs::write(&p, r#"{"hotkey":"Ctrl+Alt+Space"}"#).unwrap();
        let partial = load(&p);
        assert_eq!(partial.hotkey, "Ctrl+Alt+Space");
        assert_eq!(partial.model, "minimax-m2.7:cloud");
    }
    #[test]
    fn history_dedups_caps_at_20_most_recent_first() {
        let mut s = Settings::default();
        for i in 0..25 { s.push_history(&format!("task {i}")); }
        assert_eq!(s.history.len(), 20);
        assert_eq!(s.history[0], "task 24");
        s.push_history("task 24");
        assert_eq!(s.history.len(), 20);
        assert_eq!(s.history[0], "task 24");
        s.push_history("task 10");
        assert_eq!(s.history[0], "task 10");
        assert_eq!(s.history.iter().filter(|h| *h == "task 10").count(), 1);
    }
}
```

- [ ] **Step 2: 確認 FAIL** → **Step 3: 實作**

```rust
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum SigninState { Unknown, Yes, No }

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub hotkey: String,
    pub model: String,
    pub cautious_mode: bool,
    pub background_mode: bool,
    pub working_dir: String,
    pub autostart: bool,
    pub history: Vec<String>,
    pub signin_state: SigninState,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            hotkey: "Alt+H".into(),
            model: "minimax-m2.7:cloud".into(),
            cautious_mode: false,
            background_mode: false,
            working_dir: String::new(),
            autostart: true,
            history: Vec::new(),
            signin_state: SigninState::Unknown,
        }
    }
}
impl Settings {
    pub fn push_history(&mut self, prompt: &str) {
        self.history.retain(|h| h != prompt);
        self.history.insert(0, prompt.to_string());
        self.history.truncate(20);
    }
    pub fn effective_working_dir(&self) -> PathBuf {
        if self.working_dir.is_empty() {
            dirs::home_dir().unwrap_or_else(|| PathBuf::from("."))
        } else { PathBuf::from(&self.working_dir) }
    }
}

pub fn settings_path() -> PathBuf {
    dirs::config_dir().unwrap_or_else(|| PathBuf::from(".")).join("free-claude-code").join("settings.json")
}

pub fn load(path: &Path) -> Settings {
    std::fs::read_to_string(path).ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save(path: &Path, s: &Settings) -> std::io::Result<()> {
    if let Some(dir) = path.parent() { std::fs::create_dir_all(dir)?; }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_string_pretty(s).unwrap())?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}
```

- [ ] **Step 4: `cargo test settings::` → PASS**
- [ ] **Step 5: Commit** `feat: add settings model with atomic persistence and history`

---

### Task 4: catalog.rs(模型目錄與 fallback)

**Files:** Create: `launcher/src-tauri/src/catalog.rs`;Modify: `lib.rs`

- [ ] **Step 1: 失敗測試**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    const TAGS: &str = r#"{"models":[{"name":"minimax-m2.7"},{"name":"gpt-oss:120b"},{"name":"qwen3-coder-next"}]}"#;
    #[test]
    fn parses_api_tags_to_local_cloud_names() {
        let names = parse_cloud_models(TAGS).unwrap();
        assert_eq!(names, vec!["minimax-m2.7:cloud", "gpt-oss:120b-cloud", "qwen3-coder-next:cloud"]);
        assert!(parse_cloud_models("not json").is_none());
    }
    #[test]
    fn choose_model_prefers_configured_then_fallbacks_then_first() {
        let cat: Vec<String> = vec!["minimax-m2.7:cloud".into(), "qwen3-coder-next:cloud".into(), "glm-5:cloud".into()];
        let (m, notice) = choose_model("minimax-m2.7:cloud", &cat);
        assert_eq!(m, "minimax-m2.7:cloud"); assert!(notice.is_none());
        let (m, notice) = choose_model("dead-model:cloud", &cat);
        assert_eq!(m, "minimax-m2.7:cloud"); assert!(notice.is_some());
        let cat2: Vec<String> = vec!["glm-5:cloud".into()];
        let (m, notice) = choose_model("dead-model:cloud", &cat2);
        assert_eq!(m, "glm-5:cloud"); assert!(notice.is_some());
        let (m, notice) = choose_model("anything:cloud", &[]);
        assert_eq!(m, "anything:cloud"); assert!(notice.is_none()); // 離線:不亂改,交給 runtime
    }
}
```

- [ ] **Step 2: FAIL** → **Step 3: 實作**

```rust
/// ollama.com/api/tags 的名稱不含 cloud 後綴;經本機 daemon 使用需補:
/// 無 tag(無':')→ `name:cloud`;有 tag → `name:tag-cloud`(實證:gpt-oss:120b-cloud)。
pub fn to_local_name(catalog_name: &str) -> String {
    if catalog_name.contains(':') { format!("{catalog_name}-cloud") } else { format!("{catalog_name}:cloud") }
}

pub fn parse_cloud_models(api_tags_json: &str) -> Option<Vec<String>> {
    let v: serde_json::Value = serde_json::from_str(api_tags_json).ok()?;
    Some(v.get("models")?.as_array()?
        .iter()
        .filter_map(|m| m.get("name")?.as_str().map(to_local_name))
        .collect())
}

pub const FALLBACKS: [&str; 2] = ["minimax-m2.7:cloud", "qwen3-coder-next:cloud"];
pub const CATALOG_URL: &str = "https://ollama.com/api/tags";

/// 回傳 (要用的模型, 若有改動的中文通知)。catalog 為空(離線/未取得)時不改動。
pub fn choose_model(configured: &str, catalog: &[String]) -> (String, Option<String>) {
    if catalog.is_empty() || catalog.iter().any(|c| c == configured) {
        return (configured.to_string(), None);
    }
    let pick = FALLBACKS.iter().find(|f| catalog.iter().any(|c| c == *f))
        .map(|f| f.to_string())
        .or_else(|| catalog.first().cloned());
    match pick {
        Some(p) => {
            let notice = format!("模型 {configured} 已不在雲端目錄,改用 {p}");
            (p, Some(notice))
        }
        None => (configured.to_string(), None),
    }
}
```

- [ ] **Step 4: PASS** → **Step 5: Commit** `feat: add cloud model catalog parsing and fallback selection`

---

### Task 5: doctor.rs(體檢狀態機)

**Files:** Create: `launcher/src-tauri/src/doctor.rs`;Modify: `lib.rs`

- [ ] **Step 1: 失敗測試**

```rust
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
        let r = MockRunner::default(); // ollama 不存在 → run 回 Err
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
        let h = MockHttp::default(); // ping 永遠失敗
        let st = full_check(&deps(&r, &h, true), "minimax-m2.7:cloud");
        assert!(r.calls.lock().unwrap().iter().any(|c| c.starts_with("DETACHED ollama serve")));
        assert!(matches!(st, Status::Degraded { .. })); // 啟不起來 → Degraded
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
```

- [ ] **Step 2: FAIL** → **Step 3: 實作**

```rust
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
    /// claude.exe 候選路徑(正式環境 = PATH 查找 + %USERPROFILE%\.local\bin\claude.exe;測試注入)
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

/// 服務沒醒就拉起來;測試環境不真睡(輪詢間隔注入常數)。
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

/// 快路徑:只看檔案存在 + ping(毫秒級);細節問題交給 runtime 錯誤處理。
pub fn quick_check(deps: &Deps) -> Status {
    let ollama_present = matches!(ollama_state(deps.runner), OllamaState::Ok | OllamaState::TooOld);
    if !ollama_present || !claude_installed(&deps.claude_paths) {
        return Status::NeedsSetup { missing: vec![] }; // 呼叫端應再跑 full_check 取得清單
    }
    if !ensure_server(deps.runner, deps.http, 200, 25) {
        return Status::Degraded { reason: "Ollama 服務無法啟動".into() };
    }
    Status::Ready
}
```
(注意 `ensure_server` 真實環境輪詢 200ms×25 = 5s;單元測試走 50ms×3。)

- [ ] **Step 4: PASS** → **Step 5: Commit** `feat: add environment doctor state machine`

---

### Task 6: logging.rs + launcher.rs

**Files:** Create: `launcher/src-tauri/src/logging.rs`、`launcher/src-tauri/src/launcher.rs`;Modify: `lib.rs`

- [ ] **Step 1: 失敗測試(兩個模組)**

```rust
// logging.rs tests
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn creates_log_file_and_rotates_keeping_newest() {
        let dir = tempfile::tempdir().unwrap();
        for i in 0..35 {
            let p = dir.path().join(format!("fcc-2026061{:02}-000000.log", i % 10 + 10 * (i / 10)));
            std::fs::write(&p, "x").unwrap();
        }
        rotate(dir.path(), 30);
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 30);
        let p = new_run_log(dir.path()).unwrap();
        assert!(p.exists());
        assert!(p.file_name().unwrap().to_string_lossy().starts_with("fcc-"));
    }
}
// launcher.rs tests
#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::Settings;
    #[test]
    fn foreground_default_args() {
        let s = Settings::default();
        let spec = build_launch_spec("整理 \"桌面\" 並分類", &s, "minimax-m2.7:cloud");
        assert_eq!(spec.program, "ollama");
        assert_eq!(spec.args, vec!["launch","claude","--model","minimax-m2.7:cloud","--yes","--","--dangerously-skip-permissions","整理 \"桌面\" 並分類"]);
        assert!(!spec.background);
    }
    #[test]
    fn cautious_mode_swaps_permission_flag() {
        let mut s = Settings::default(); s.cautious_mode = true;
        let spec = build_launch_spec("p", &s, "m");
        assert_eq!(spec.args, vec!["launch","claude","--model","m","--yes","--","--permission-mode","acceptEdits","p"]);
    }
    #[test]
    fn background_mode_adds_print_flag() {
        let mut s = Settings::default(); s.background_mode = true;
        let spec = build_launch_spec("p", &s, "m");
        assert_eq!(spec.args, vec!["launch","claude","--model","m","--yes","--","-p","--dangerously-skip-permissions","p"]);
        assert!(spec.background);
    }
    #[test]
    fn working_dir_defaults_to_home() {
        let spec = build_launch_spec("p", &Settings::default(), "m");
        assert_eq!(spec.cwd, dirs::home_dir().unwrap());
        let mut s = Settings::default(); s.working_dir = "C:\\Temp".into();
        assert_eq!(build_launch_spec("p", &s, "m").cwd.to_string_lossy(), "C:\\Temp");
    }
}
```

- [ ] **Step 2: FAIL** → **Step 3: 實作**

```rust
// logging.rs
use std::path::{Path, PathBuf};

pub fn logs_dir() -> PathBuf {
    dirs::config_dir().unwrap_or_else(|| PathBuf::from(".")).join("free-claude-code").join("logs")
}

pub fn new_run_log(dir: &Path) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
    let mut path = dir.join(format!("fcc-{stamp}.log"));
    let mut n = 1;
    while path.exists() { path = dir.join(format!("fcc-{stamp}-{n}.log")); n += 1; }
    std::fs::write(&path, "")?;
    Ok(path)
}

pub fn rotate(dir: &Path, keep: usize) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let mut files: Vec<PathBuf> = rd.flatten().map(|e| e.path())
        .filter(|p| p.file_name().map_or(false, |n| n.to_string_lossy().starts_with("fcc-"))).collect();
    files.sort(); // 檔名含時間戳,字典序 = 時間序
    if files.len() > keep {
        let excess = files.len() - keep;
        for p in files.into_iter().take(excess) { let _ = std::fs::remove_file(p); }
    }
}
```

```rust
// launcher.rs
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
    let mut args: Vec<String> = vec!["launch".into(), "claude".into(), "--model".into(), model.into(), "--yes".into(), "--".into()];
    if s.background_mode { args.push("-p".into()); }
    if s.cautious_mode {
        args.push("--permission-mode".into());
        args.push("acceptEdits".into());
    } else {
        args.push("--dangerously-skip-permissions".into());
    }
    args.push(prompt.to_string());
    LaunchSpec { program: "ollama".into(), args, cwd: s.effective_working_dir(), background: s.background_mode }
}

#[cfg(windows)]
const CREATE_NEW_CONSOLE: u32 = 0x0000_0010;
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// 前景:獨立主控台視窗(Win11 會依系統預設導向 Windows Terminal)。
/// 背景:無視窗,stdout/stderr 寫入 log,結束時呼叫 on_done(exit_code, log_path)。
pub fn spawn(spec: &LaunchSpec, log_path: PathBuf, on_done: Option<Box<dyn FnOnce(i32, PathBuf) + Send>>) -> std::io::Result<()> {
    use std::os::windows::process::CommandExt;
    use std::process::{Command, Stdio};
    let mut cmd = Command::new(&spec.program);
    cmd.args(&spec.args).current_dir(&spec.cwd);
    if spec.background {
        let log = std::fs::File::create(&log_path)?;
        let log2 = log.try_clone()?;
        cmd.creation_flags(CREATE_NO_WINDOW).stdin(Stdio::null()).stdout(log).stderr(log2);
        let mut child = cmd.spawn()?;
        std::thread::spawn(move || {
            let code = child.wait().map(|s| s.code().unwrap_or(-1)).unwrap_or(-1);
            if let Some(f) = on_done { f(code, log_path); }
        });
    } else {
        cmd.creation_flags(CREATE_NEW_CONSOLE).spawn()?;
    }
    Ok(())
}
```

- [ ] **Step 4: PASS**(`cargo test logging:: launcher::`)
- [ ] **Step 5: Commit** `feat: add run logging and launch command builder with spawn`

---

### Task 7: bootstrap.rs(精靈安裝步驟)

**Files:** Create: `launcher/src-tauri/src/bootstrap.rs`;Modify: `lib.rs`

- [ ] **Step 1: 失敗測試**

```rust
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
        assert!(calls.iter().any(|c| c.contains("install -e --id Ollama.Ollama --scope user")));
    }
    #[test]
    fn ollama_install_falls_back_to_direct_download_when_winget_missing() {
        let r = MockRunner::default(); // winget 不存在
        let h = MockHttp::default().on(OLLAMA_SETUP_URL, Ok("fake-installer-bytes"));
        let dir = tempfile::tempdir().unwrap();
        let res = install_ollama(&r, &h, dir.path().to_path_buf());
        // MockRunner 沒設定 OllamaSetup.exe → run Err → res.ok == false,但必須已下載檔案且嘗試執行
        assert!(!res.ok);
        assert!(dir.path().join("OllamaSetup.exe").exists());
        assert!(r.calls.lock().unwrap().iter().any(|c| c.contains("OllamaSetup.exe /VERYSILENT")));
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
```

- [ ] **Step 2: FAIL** → **Step 3: 實作**

```rust
use crate::command::{Runner, CmdOutput};
use crate::http::Http;
use std::path::PathBuf;
use std::time::Duration;

#[derive(Debug, Clone, serde::Serialize)]
pub struct StepResult { pub ok: bool, pub detail: String }

fn result(r: std::io::Result<CmdOutput>, action: &str) -> StepResult {
    match r {
        Ok(o) if o.ok() => StepResult { ok: true, detail: format!("{action} 完成") },
        Ok(o) => StepResult { ok: false, detail: format!("{action} 失敗 (exit {}): {}", o.code, truncate(&o.stderr, 400)) },
        Err(e) => StepResult { ok: false, detail: format!("{action} 失敗: {e}") },
    }
}
fn truncate(s: &str, n: usize) -> String { s.chars().take(n).collect() }

pub const OLLAMA_SETUP_URL: &str = "https://ollama.com/download/OllamaSetup.exe";
const LONG: Duration = Duration::from_secs(1800);

fn winget_path(runner: &dyn Runner) -> Option<String> {
    if runner.run("winget", &["--version"], Duration::from_secs(10)).map(|o| o.ok()).unwrap_or(false) {
        return Some("winget".into());
    }
    let local = dirs::data_local_dir()?.join("Microsoft").join("WindowsApps").join("winget.exe");
    if local.exists() { return Some(local.to_string_lossy().into_owned()); }
    None
}

pub fn install_ollama(runner: &dyn Runner, http: &dyn Http, temp_dir: PathBuf) -> StepResult {
    if let Some(winget) = winget_path(runner) {
        let r = runner.run(&winget, &["install","-e","--id","Ollama.Ollama","--scope","user","--accept-source-agreements","--accept-package-agreements"], LONG);
        if let Ok(ref o) = r { if o.ok() { refresh_path(); return result(r, "安裝 Ollama (winget)"); } }
    }
    // fallback:直接下載官方安裝器靜默安裝(Inno Setup → /VERYSILENT)
    match http.get_bytes(OLLAMA_SETUP_URL, Duration::from_secs(600)) {
        Err(e) => StepResult { ok: false, detail: format!("下載 OllamaSetup.exe 失敗: {e}") },
        Ok(bytes) => {
            let exe = temp_dir.join("OllamaSetup.exe");
            if let Err(e) = std::fs::write(&exe, bytes) {
                return StepResult { ok: false, detail: format!("寫入安裝器失敗: {e}") };
            }
            let r = runner.run(&exe.to_string_lossy(), &["/VERYSILENT","/SP-","/SUPPRESSMSGBOXES"], LONG);
            refresh_path();
            result(r, "安裝 Ollama (直接下載)")
        }
    }
}

pub fn install_claude(runner: &dyn Runner) -> StepResult {
    let r = runner.run("powershell", &["-NoProfile","-ExecutionPolicy","Bypass","-Command","irm https://claude.ai/install.ps1 | iex"], LONG);
    refresh_path();
    result(r, "安裝 Claude Code")
}

/// `ollama signin` 會開瀏覽器並等待配對完成;不設短 timeout(上限 10 分鐘)。
pub fn signin(runner: &dyn Runner) -> StepResult {
    result(runner.run("ollama", &["signin"], Duration::from_secs(600)), "登入 ollama.com")
}

pub fn register_model(runner: &dyn Runner, model: &str) -> StepResult {
    result(runner.run("ollama", &["pull", model], Duration::from_secs(300)), "註冊雲端模型")
}

/// 安裝後讓「目前程序」看得到新 PATH:重讀註冊表 + 明確補已知安裝目錄。
pub fn refresh_path() {
    let mut parts: Vec<PathBuf> = Vec::new();
    if let Some(home) = dirs::home_dir() { parts.push(home.join(".local").join("bin")); }
    if let Some(local) = dirs::data_local_dir() { parts.push(local.join("Programs").join("Ollama")); }
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
```

- [ ] **Step 4: PASS** → **Step 5: Commit** `feat: add bootstrap install steps with winget fallback`

---

### Task 8: 後端組裝(lib.rs + ipc.rs + tauri.conf.json)

**Files:**
- Create: `launcher/src-tauri/src/ipc.rs`
- Modify: `launcher/src-tauri/src/lib.rs`、`launcher/src-tauri/src/main.rs`、`launcher/src-tauri/tauri.conf.json`、`launcher/src-tauri/capabilities/default.json`

本任務以「編譯通過 + `npm run tauri dev` 手動煙霧測試」驗證(無純邏輯可單測;邏輯都在前面模組)。

- [ ] **Step 1: tauri.conf.json 設定三個視窗與基本資訊**

`app.windows`:
```json
[
  { "label": "palette", "title": "Free Claude Code", "width": 640, "height": 168, "visible": false, "decorations": false, "alwaysOnTop": true, "skipTaskbar": true, "resizable": false, "center": false },
  { "label": "wizard", "title": "Free Claude Code 首次設定", "width": 560, "height": 540, "visible": false, "resizable": false },
  { "label": "settings", "title": "Free Claude Code 設定", "width": 560, "height": 600, "visible": false, "resizable": false }
]
```
`productName`: `Free Claude Code`;`identifier`: `com.jaylooloomi.free-claude-code`;`app.trayIcon` 留待 lib.rs 程式建立。

- [ ] **Step 2: ipc.rs — 全部 tauri commands**

```rust
use crate::{bootstrap, catalog, doctor, launcher, logging, settings};
use crate::command::SystemRunner;
use crate::http::UreqHttp;
use crate::settings::{Settings, SigninState};
use std::sync::Mutex;
use tauri::{AppHandle, Manager, State};

pub struct AppState {
    pub settings: Mutex<Settings>,
    pub pending_prompt: Mutex<Option<String>>,
    pub catalog_cache: Mutex<Vec<String>>,
}

fn deps_claude_paths() -> Vec<std::path::PathBuf> { doctor::default_claude_paths() }

#[derive(serde::Serialize)]
pub struct StatusDto { pub state: String, pub model: String, pub detail: String }

#[tauri::command]
pub fn get_settings(state: State<AppState>) -> Settings { state.settings.lock().unwrap().clone() }

#[tauri::command]
pub fn save_settings(app: AppHandle, state: State<AppState>, new_settings: Settings) -> Result<(), String> {
    let hotkey_changed;
    {
        let mut s = state.settings.lock().unwrap();
        hotkey_changed = s.hotkey != new_settings.hotkey;
        *s = new_settings.clone();
        settings::save(&settings::settings_path(), &s).map_err(|e| e.to_string())?;
    }
    if hotkey_changed { crate::register_hotkey(&app, &new_settings.hotkey)?; }
    crate::sync_autostart(&app, new_settings.autostart);
    Ok(())
}

#[tauri::command]
pub fn get_status(state: State<AppState>) -> StatusDto {
    let s = state.settings.lock().unwrap().clone();
    let runner = SystemRunner; let http = UreqHttp;
    let deps = doctor::Deps { runner: &runner, http: &http, claude_paths: deps_claude_paths() };
    let cat = state.catalog_cache.lock().unwrap().clone();
    let (model, _) = catalog::choose_model(&s.model, &cat);
    match doctor::quick_check(&deps) {
        doctor::Status::Ready => StatusDto { state: "ready".into(), model, detail: String::new() },
        doctor::Status::NeedsSetup { .. } => StatusDto { state: "needs_setup".into(), model, detail: "首次使用將自動安裝必要元件".into() },
        doctor::Status::Degraded { reason } => StatusDto { state: "degraded".into(), model, detail: reason },
    }
}

#[tauri::command]
pub fn get_history(state: State<AppState>) -> Vec<String> { state.settings.lock().unwrap().history.clone() }

/// 回傳 "launched" | "wizard";Err(中文訊息) 顯示在面板。
#[tauri::command]
pub fn submit_prompt(app: AppHandle, state: State<AppState>, prompt: String) -> Result<String, String> {
    let prompt = prompt.trim().to_string();
    if prompt.is_empty() { return Err("請輸入需求".into()); }
    {
        let mut s = state.settings.lock().unwrap();
        s.push_history(&prompt);
        let _ = settings::save(&settings::settings_path(), &s);
    }
    let runner = SystemRunner; let http = UreqHttp;
    let deps = doctor::Deps { runner: &runner, http: &http, claude_paths: deps_claude_paths() };
    match doctor::quick_check(&deps) {
        doctor::Status::NeedsSetup { .. } => {
            *state.pending_prompt.lock().unwrap() = Some(prompt);
            show_window(&app, "wizard");
            hide_window(&app, "palette");
            Ok("wizard".into())
        }
        doctor::Status::Degraded { reason } => Err(reason),
        doctor::Status::Ready => { do_launch(&app, &state, &prompt)?; hide_window(&app, "palette"); Ok("launched".into()) }
    }
}

pub fn do_launch(app: &AppHandle, state: &State<AppState>, prompt: &str) -> Result<(), String> {
    let s = state.settings.lock().unwrap().clone();
    let cat = state.catalog_cache.lock().unwrap().clone();
    let (model, notice) = catalog::choose_model(&s.model, &cat);
    if let Some(n) = notice { crate::notify(app, &n); }
    let spec = launcher::build_launch_spec(prompt, &s, &model);
    let dir = logging::logs_dir();
    logging::rotate(&dir, 30);
    let log = logging::new_run_log(&dir).map_err(|e| e.to_string())?;
    let app2 = app.clone();
    let on_done: Option<Box<dyn FnOnce(i32, std::path::PathBuf) + Send>> = if spec.background {
        Some(Box::new(move |code, log_path| {
            let msg = if code == 0 { "任務完成".to_string() } else { format!("任務失敗 (exit {code}),記錄:{}", log_path.display()) };
            crate::notify(&app2, &msg);
        }))
    } else { None };
    launcher::spawn(&spec, log, on_done).map_err(|e| e.to_string())?;
    // 成功啟動過 → 視為已登入(auth 失敗時 runtime 會改回 No)
    let mut st = state.settings.lock().unwrap();
    if st.signin_state == SigninState::Unknown { st.signin_state = SigninState::Yes; let _ = settings::save(&settings::settings_path(), &st); }
    Ok(())
}

#[derive(serde::Serialize)]
pub struct WizardPlan { pub steps: Vec<String> }

#[tauri::command]
pub fn wizard_plan(state: State<AppState>) -> WizardPlan {
    let runner = SystemRunner; let http = UreqHttp;
    let deps = doctor::Deps { runner: &runner, http: &http, claude_paths: deps_claude_paths() };
    let model = state.settings.lock().unwrap().model.clone();
    let mut steps: Vec<String> = Vec::new();
    if let doctor::Status::NeedsSetup { missing } = doctor::full_check(&deps, &model) {
        for c in missing {
            steps.push(match c {
                doctor::Component::Ollama => "ollama",
                doctor::Component::OllamaUpgrade => "ollama_upgrade",
                doctor::Component::ClaudeCode => "claude",
            }.to_string());
        }
    }
    let signed = state.settings.lock().unwrap().signin_state == SigninState::Yes;
    if !signed { steps.push("signin".into()); }
    steps.push("model".into());
    WizardPlan { steps }
}

#[tauri::command]
pub async fn wizard_run(state: State<'_, AppState>, step: String) -> Result<bootstrap::StepResult, String> {
    let runner = SystemRunner; let http = UreqHttp;
    let model = state.settings.lock().unwrap().model.clone();
    let res = tauri::async_runtime::spawn_blocking(move || match step.as_str() {
        "ollama" | "ollama_upgrade" => bootstrap::install_ollama(&runner, &http, std::env::temp_dir()),
        "claude" => bootstrap::install_claude(&runner),
        "signin" => bootstrap::signin(&runner),
        "model" => bootstrap::register_model(&runner, &model),
        other => bootstrap::StepResult { ok: false, detail: format!("未知步驟 {other}") },
    }).await.map_err(|e| e.to_string())?;
    if res.ok {
        let mut s = state.settings.lock().unwrap();
        s.signin_state = SigninState::Yes.clone();
        let _ = settings::save(&settings::settings_path(), &s);
    }
    Ok(res)
}
```
(注意:`wizard_run` 中 signin 成功才標 `Yes` — 修正:只在 `step == "signin"` 時更新;實作時用 `if step == "signin" && res.ok` 條件。)

```rust
#[tauri::command]
pub fn wizard_done(app: AppHandle, state: State<AppState>) -> Result<(), String> {
    hide_window(&app, "wizard");
    let pending = state.pending_prompt.lock().unwrap().take();
    if let Some(p) = pending { do_launch(&app, &state, &p)?; }
    Ok(())
}

#[tauri::command]
pub fn list_cloud_models(state: State<AppState>) -> Vec<String> { state.catalog_cache.lock().unwrap().clone() }

#[tauri::command]
pub fn open_logs() -> Result<(), String> {
    let dir = logging::logs_dir();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    crate::command::SystemRunner.spawn_detached("explorer", &[&dir.to_string_lossy()]).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn hide_palette(app: AppHandle) { hide_window(&app, "palette"); }
#[tauri::command]
pub fn open_settings_window(app: AppHandle) { show_window(&app, "settings"); }

pub fn show_window(app: &AppHandle, label: &str) {
    if let Some(w) = app.get_webview_window(label) { let _ = w.show(); let _ = w.set_focus(); }
}
pub fn hide_window(app: &AppHandle, label: &str) {
    if let Some(w) = app.get_webview_window(label) { let _ = w.hide(); }
}
```

- [ ] **Step 3: lib.rs 組裝**

```rust
pub mod bootstrap; pub mod catalog; pub mod command; pub mod doctor;
pub mod http; pub mod ipc; pub mod launcher; pub mod logging;
pub mod settings; pub mod version;

use ipc::AppState;
use std::sync::Mutex;
use tauri::{AppHandle, Manager};
use tauri_plugin_autostart::ManagerExt as _;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

pub fn notify(app: &AppHandle, body: &str) {
    use tauri_plugin_notification::NotificationExt;
    let _ = app.notification().builder().title("Free Claude Code").body(body).show();
}

pub fn register_hotkey(app: &AppHandle, hotkey: &str) -> Result<(), String> {
    let gs = app.global_shortcut();
    let _ = gs.unregister_all();
    gs.on_shortcut(hotkey, |app, _shortcut, event| {
        if event.state() == ShortcutState::Pressed { show_palette_centered(app); }
    }).map_err(|e| format!("快捷鍵註冊失敗({hotkey}):{e}"))
}

pub fn sync_autostart(app: &AppHandle, enabled: bool) {
    let am = app.autolaunch();
    let _ = if enabled { am.enable() } else { am.disable() };
}

fn show_palette_centered(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("palette") {
        if let Ok(Some(monitor)) = w.current_monitor() {
            let ms = monitor.size();
            let ws = w.outer_size().unwrap_or(tauri::PhysicalSize { width: 640, height: 168 });
            let x = (ms.width.saturating_sub(ws.width)) / 2;
            let y = ms.height / 4;
            let _ = w.set_position(tauri::PhysicalPosition { x: x as i32, y: y as i32 });
        }
        let _ = w.show(); let _ = w.set_focus();
        let _ = w.emit("palette-shown", ());
    }
}

fn handle_argv(app: &AppHandle, argv: &[String]) {
    if let Some(i) = argv.iter().position(|a| a == "--run") {
        if let Some(prompt) = argv.get(i + 1) {
            let state = app.state::<AppState>();
            let _ = ipc::submit_prompt(app.clone(), state, prompt.clone());
            return;
        }
    }
    if argv.iter().any(|a| a == "--show-palette") { show_palette_centered(app); }
}

fn refresh_catalog(app: AppHandle) {
    tauri::async_runtime::spawn_blocking(move || {
        use crate::http::Http;
        let http = http::UreqHttp;
        if let Ok(json) = http.get(catalog::CATALOG_URL, std::time::Duration::from_secs(10)) {
            if let Some(models) = catalog::parse_cloud_models(&json) {
                let state = app.state::<AppState>();
                *state.catalog_cache.lock().unwrap() = models;
            }
        }
    });
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let loaded = settings::load(&settings::settings_path());
    tauri::Builder::default()
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_autostart::init(tauri_plugin_autostart::MacosLauncher::LaunchAgent, None))
        .plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| { handle_argv(app, &argv); }))
        .manage(AppState {
            settings: Mutex::new(loaded.clone()),
            pending_prompt: Mutex::new(None),
            catalog_cache: Mutex::new(Vec::new()),
        })
        .invoke_handler(tauri::generate_handler![
            ipc::get_settings, ipc::save_settings, ipc::get_status, ipc::get_history,
            ipc::submit_prompt, ipc::wizard_plan, ipc::wizard_run, ipc::wizard_done,
            ipc::list_cloud_models, ipc::open_logs, ipc::hide_palette, ipc::open_settings_window
        ])
        .setup(move |app| {
            let handle = app.handle().clone();
            if let Err(e) = register_hotkey(&handle, &loaded.hotkey) {
                notify(&handle, &e);
                ipc::show_window(&handle, "settings");
            }
            sync_autostart(&handle, loaded.autostart);
            refresh_catalog(handle.clone());
            build_tray(&handle)?;
            let argv: Vec<String> = std::env::args().collect();
            handle_argv(&handle, &argv);
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    use tauri::menu::{Menu, MenuItem};
    use tauri::tray::TrayIconBuilder;
    let open = MenuItem::with_id(app, "open", "開啟輸入面板", true, None::<&str>)?;
    let settings_item = MenuItem::with_id(app, "settings", "設定", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "結束", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &settings_item, &quit])?;
    TrayIconBuilder::with_id("main")
        .icon(app.default_window_icon().unwrap().clone())
        .menu(&menu)
        .tooltip("Free Claude Code")
        .on_menu_event(|app, e| match e.id.as_ref() {
            "open" => show_palette_centered(app),
            "settings" => ipc::show_window(app, "settings"),
            "quit" => app.exit(0),
            _ => {}
        })
        .build(app)?;
    Ok(())
}
```
(外掛確切 API 名稱以編譯器為準;palette 的 Esc/blur 隱藏在前端做。)

- [ ] **Step 4: capabilities/default.json 加權限**(global-shortcut、notification、autostart、core:window、core:event、core:tray 對三個視窗開放)

- [ ] **Step 5: `cargo check` + `cargo test` 全綠;`npm run tauri dev` 啟動後:系統匣圖示存在、`Alt+H` 彈出空白 palette 視窗(UI 下一任務)**

- [ ] **Step 6: Commit** `feat: wire tray, hotkey, windows, single-instance and IPC commands`

---

### Task 9: Palette UI

**Files:**
- Create: `launcher/src/lib/strings.ts`、`launcher/src/lib/api.ts`、`launcher/src/lib/Palette.svelte`
- Modify: `launcher/src/App.svelte`、`launcher/src/main.ts`、`launcher/src/app.css`(scaffold 樣式清掉換深色簡潔風)

- [ ] **Step 1: strings.ts**

```ts
export const S = {
  placeholder: "想要我做什麼?(例:幫我整理桌面,並且建立資料夾分類)",
  statusReady: (model: string) => `就緒 · ${model}`,
  statusNeedsSetup: "首次使用:送出後將自動安裝必要元件",
  statusDegraded: (d: string) => `注意:${d}`,
  statusOffline: "離線 — 雲端模型需要網路連線",
  launched: "已啟動,可關閉此面板",
  empty: "請輸入需求",
};
```

- [ ] **Step 2: api.ts**

```ts
import { invoke } from "@tauri-apps/api/core";

export interface Settings {
  hotkey: string; model: string; cautious_mode: boolean; background_mode: boolean;
  working_dir: string; autostart: boolean; history: string[]; signin_state: string;
}
export interface StatusDto { state: "ready" | "needs_setup" | "degraded"; model: string; detail: string }
export interface StepResult { ok: boolean; detail: string }

export const api = {
  getSettings: () => invoke<Settings>("get_settings"),
  saveSettings: (newSettings: Settings) => invoke<void>("save_settings", { newSettings }),
  getStatus: () => invoke<StatusDto>("get_status"),
  getHistory: () => invoke<string[]>("get_history"),
  submitPrompt: (prompt: string) => invoke<string>("submit_prompt", { prompt }),
  wizardPlan: () => invoke<{ steps: string[] }>("wizard_plan"),
  wizardRun: (step: string) => invoke<StepResult>("wizard_run", { step }),
  wizardDone: () => invoke<void>("wizard_done"),
  listCloudModels: () => invoke<string[]>("list_cloud_models"),
  openLogs: () => invoke<void>("open_logs"),
  hidePalette: () => invoke<void>("hide_palette"),
};
```

- [ ] **Step 3: App.svelte 依視窗 label 路由**

```svelte
<script lang="ts">
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import Palette from "./lib/Palette.svelte";
  import Wizard from "./lib/Wizard.svelte";
  import Settings from "./lib/Settings.svelte";
  const label = getCurrentWindow().label;
</script>
{#if label === "palette"}<Palette />{:else if label === "wizard"}<Wizard />{:else}<Settings />{/if}
```
(Wizard/Settings 先建立空殼元件 `<div>稍後實作</div>` 讓編譯通過,於 Task 10/11 填入。)

- [ ] **Step 4: Palette.svelte**

```svelte
<script lang="ts">
  import { onMount } from "svelte";
  import { listen } from "@tauri-apps/api/event";
  import { api, type StatusDto } from "./api";
  import { S } from "./strings";

  let input = $state("");
  let status = $state<StatusDto | null>(null);
  let error = $state("");
  let busy = $state(false);
  let history: string[] = $state([]);
  let hIndex = $state(-1);
  let el: HTMLInputElement;

  async function refresh() {
    history = await api.getHistory();
    try { status = await api.getStatus(); } catch { status = null; }
  }
  onMount(() => {
    refresh();
    listen("palette-shown", () => { input = ""; error = ""; hIndex = -1; refresh(); el?.focus(); });
    window.addEventListener("blur", () => api.hidePalette());
  });

  async function submit() {
    if (busy) return;
    error = "";
    busy = true;
    try { await api.submitPrompt(input); }
    catch (e) { error = String(e); }
    finally { busy = false; }
  }
  function onKey(e: KeyboardEvent) {
    if (e.key === "Enter") submit();
    else if (e.key === "Escape") api.hidePalette();
    else if (e.key === "ArrowUp" && hIndex < history.length - 1) { hIndex += 1; input = history[hIndex]; }
    else if (e.key === "ArrowDown" && hIndex > -1) { hIndex -= 1; input = hIndex === -1 ? "" : history[hIndex]; }
  }
  const statusText = $derived(!status ? "" :
    status.state === "ready" ? S.statusReady(status.model) :
    status.state === "needs_setup" ? S.statusNeedsSetup : S.statusDegraded(status.detail));
</script>

<main class="palette">
  <input bind:this={el} bind:value={input} placeholder={S.placeholder} onkeydown={onKey} disabled={busy} autofocus />
  <div class="status" class:error={!!error}>{error || statusText}</div>
</main>

<style>
  .palette { display: flex; flex-direction: column; gap: 8px; padding: 16px; }
  input { font-size: 18px; padding: 12px 14px; border-radius: 8px; border: 1px solid #444;
          background: #1e1e1e; color: #eee; outline: none; }
  input:focus { border-color: #7aa2f7; }
  .status { font-size: 12px; color: #999; min-height: 16px; }
  .status.error { color: #f7768e; }
</style>
```
(scaffold 的 `app.css` 改為:`html,body{margin:0;background:#181818;} *{box-sizing:border-box;font-family:"Segoe UI",sans-serif;}`)

- [ ] **Step 5: 手動驗證**:`npm run tauri dev` → `Alt+H` 喚出 → 打字 → Enter:(a) 環境就緒時開出終端機跑 `ollama launch`、面板關閉;(b) Esc 與失焦會隱藏;(c) ↑ 叫回歷史。
- [ ] **Step 6: Commit** `feat: add palette UI with history and status bar`

---

### Task 10: Wizard UI(含暫存需求自動續跑)

**Files:** Create: `launcher/src/lib/Wizard.svelte`(取代空殼)

- [ ] **Step 1: 實作 Wizard.svelte**

```svelte
<script lang="ts">
  import { onMount } from "svelte";
  import { api, type StepResult } from "./api";

  const LABELS: Record<string, string> = {
    ollama: "安裝 Ollama", ollama_upgrade: "升級 Ollama",
    claude: "安裝 Claude Code", signin: "登入 ollama.com", model: "註冊雲端模型",
  };
  type Row = { step: string; state: "pending" | "running" | "ok" | "fail"; detail: string };
  let rows: Row[] = $state([]);
  let done = $state(false);
  let failed = $state(false);

  onMount(async () => {
    const plan = await api.wizardPlan();
    rows = plan.steps.map((step) => ({ step, state: "pending", detail: "" }));
    runFrom(0);
  });

  async function runFrom(start: number) {
    failed = false;
    for (let i = start; i < rows.length; i++) {
      if (rows[i].step === "signin" && rows[i].state === "pending" && !autoSignin) {
        rows[i].state = "pending"; // 等使用者按鈕
        signinIndex = i;
        return;
      }
      rows[i].state = "running";
      let r: StepResult;
      try { r = await api.wizardRun(rows[i].step); }
      catch (e) { r = { ok: false, detail: String(e) }; }
      rows[i].state = r.ok ? "ok" : "fail";
      rows[i].detail = r.detail;
      if (!r.ok) { failed = true; return; }
    }
    done = true;
  }
  let signinIndex = $state(-1);
  let autoSignin = $state(false);
  async function startSignin() { autoSignin = true; runFrom(signinIndex); }
  function retry() { const i = rows.findIndex((r) => r.state === "fail"); if (i >= 0) { rows[i].state = "pending"; runFrom(i); } }
  async function finish() { await api.wizardDone(); }
</script>

<main class="wizard">
  <h1>首次設定</h1>
  <p class="hint">以下元件將自動安裝(皆免系統管理員權限)</p>
  <ul>
    {#each rows as row, i}
      <li class={row.state}>
        <span class="mark">{row.state === "ok" ? "✓" : row.state === "fail" ? "✗" : row.state === "running" ? "…" : "·"}</span>
        {LABELS[row.step] ?? row.step}
        {#if row.step === "signin" && i === signinIndex && row.state === "pending"}
          <button onclick={startSignin}>開啟瀏覽器登入</button>
        {/if}
        {#if row.detail && row.state === "fail"}<div class="detail">{row.detail}</div>{/if}
      </li>
    {/each}
  </ul>
  {#if failed}<button onclick={retry}>重試失敗的步驟</button>{/if}
  {#if done}
    <div class="disclaimer">本工具會讓 AI 自動執行檔案操作(預設不逐項確認)。可在設定中開啟「謹慎模式」。</div>
    <button class="primary" onclick={finish}>開始使用</button>
  {/if}
</main>

<style>
  .wizard { padding: 24px; color: #eee; }
  h1 { font-size: 20px; margin: 0 0 4px; }
  .hint { color: #999; font-size: 13px; }
  ul { list-style: none; padding: 0; display: flex; flex-direction: column; gap: 10px; }
  li { font-size: 15px; } li.ok .mark { color: #9ece6a; } li.fail .mark { color: #f7768e; }
  li.running .mark { color: #7aa2f7; }
  .mark { display: inline-block; width: 20px; }
  .detail { color: #f7768e; font-size: 12px; margin-left: 20px; white-space: pre-wrap; }
  .disclaimer { background: #2a2a2a; border-radius: 8px; padding: 12px; font-size: 13px; color: #ccc; margin: 12px 0; }
  button { padding: 8px 14px; border-radius: 8px; border: 1px solid #444; background: #2a2a2a; color: #eee; cursor: pointer; }
  button.primary { background: #7aa2f7; color: #111; border: none; }
</style>
```

- [ ] **Step 2: 手動驗證**(模擬缺件:暫時把 doctor 的 claude_paths 改成不存在路徑跑 dev,或直接在乾淨沙盒驗證 — 以 Task 16 為準;本步驟驗證 UI 流程與 signin 按鈕邏輯)
- [ ] **Step 3: Commit** `feat: add first-run wizard UI with auto-resume of stashed prompt`

---

### Task 11: Settings UI

**Files:** Create: `launcher/src/lib/Settings.svelte`(取代空殼)

- [ ] **Step 1: 實作**

```svelte
<script lang="ts">
  import { onMount } from "svelte";
  import { api, type Settings } from "./api";

  let s: Settings | null = $state(null);
  let models: string[] = $state([]);
  let saved = $state(false);
  let error = $state("");

  onMount(async () => {
    s = await api.getSettings();
    models = await api.listCloudModels();
    if (s && !models.includes(s.model)) models = [s.model, ...models];
  });
  async function save() {
    if (!s) return;
    error = ""; saved = false;
    try { await api.saveSettings(s); saved = true; setTimeout(() => (saved = false), 1500); }
    catch (e) { error = String(e); }
  }
</script>

{#if s}
<main class="settings">
  <h1>設定</h1>
  <label>快捷鍵 <input bind:value={s.hotkey} placeholder="Alt+H" />
    <small>格式如 Alt+H、Ctrl+Alt+Space。注意:Alt+H 與 Office 功能區快速鍵衝突。</small></label>
  <label>模型
    <select bind:value={s.model}>{#each models as m}<option value={m}>{m}</option>{/each}</select></label>
  <label class="row"><input type="checkbox" bind:checked={s.cautious_mode} /> 謹慎模式(危險操作前詢問,取代完全自動)</label>
  <label class="row"><input type="checkbox" bind:checked={s.background_mode} /> 背景模式(不開終端機,完成後通知)</label>
  <label>工作目錄 <input bind:value={s.working_dir} placeholder="留空 = 使用者資料夾" /></label>
  <label class="row"><input type="checkbox" bind:checked={s.autostart} /> 開機自動啟動</label>
  <div class="actions">
    <button class="primary" onclick={save}>儲存</button>
    <button onclick={() => api.openLogs()}>開啟記錄資料夾</button>
    {#if saved}<span class="ok">已儲存 ✓</span>{/if}
    {#if error}<span class="err">{error}</span>{/if}
  </div>
</main>
{/if}

<style>
  .settings { padding: 24px; color: #eee; display: flex; flex-direction: column; gap: 14px; }
  h1 { font-size: 20px; margin: 0; }
  label { display: flex; flex-direction: column; gap: 4px; font-size: 14px; }
  label.row { flex-direction: row; align-items: center; gap: 8px; }
  input:not([type="checkbox"]), select { padding: 8px; border-radius: 6px; border: 1px solid #444; background: #1e1e1e; color: #eee; }
  small { color: #888; }
  .actions { display: flex; gap: 10px; align-items: center; }
  button { padding: 8px 14px; border-radius: 8px; border: 1px solid #444; background: #2a2a2a; color: #eee; cursor: pointer; }
  button.primary { background: #7aa2f7; color: #111; border: none; }
  .ok { color: #9ece6a; } .err { color: #f7768e; font-size: 12px; }
</style>
```

- [ ] **Step 2: 手動驗證**:dev 模式開設定 → 改快捷鍵成 `Ctrl+Alt+Space` 儲存 → 新快捷鍵立即生效、舊的失效;改回 `Alt+H`。勾背景模式 → 送出任務 → 不開終端機、完成跳通知。
- [ ] **Step 3: Commit** `feat: add settings UI with live hotkey re-registration`

---

### Task 12: 錯誤情境收斂(429/auth/offline)

**Files:** Modify: `launcher/src-tauri/src/launcher.rs`、`launcher/src-tauri/src/ipc.rs`

- [ ] **Step 1: 失敗測試(launcher.rs 增加輸出分類器)**

```rust
#[test]
fn classifies_runtime_errors_from_log_tail() {
    assert_eq!(classify_failure("... 429 Too Many Requests ..."), FailureKind::Quota);
    assert_eq!(classify_failure("...usage limit reached..."), FailureKind::Quota);
    assert_eq!(classify_failure("... 401 Unauthorized ..."), FailureKind::Auth);
    assert_eq!(classify_failure("...not signed in..."), FailureKind::Auth);
    assert_eq!(classify_failure("random crash"), FailureKind::Other);
}
```

- [ ] **Step 2: FAIL** → **Step 3: 實作**

```rust
#[derive(Debug, PartialEq)]
pub enum FailureKind { Quota, Auth, Other }

pub fn classify_failure(log_tail: &str) -> FailureKind {
    let t = log_tail.to_lowercase();
    if t.contains("429") || t.contains("usage limit") || t.contains("quota") || t.contains("rate limit") { FailureKind::Quota }
    else if t.contains("401") || t.contains("unauthorized") || t.contains("not signed in") || t.contains("signin") { FailureKind::Auth }
    else { FailureKind::Other }
}
```

- [ ] **Step 4: ipc.rs 背景完成 callback 套用分類**:讀 log 尾 4KB → `Quota` → 通知「免費額度已用完,稍後重置(限制綁帳號,換模型無效)」;`Auth` → `signin_state = No` 存檔 + 通知「需要重新登入,下次啟動會引導」;`Other` → 通知失敗 + log 路徑。`submit_prompt` 在 `signin_state == No` 時直接走 wizard 路徑。
- [ ] **Step 5: `cargo test` 全綠** → **Step 6: Commit** `feat: classify runtime failures and route auth errors to wizard`

---

### Task 13: 打包(icons、NSIS、WebView2)

**Files:** Modify: `launcher/src-tauri/tauri.conf.json`;Create: `launcher/src-tauri/icons/*`(由 `tauri icon` 產生)

- [ ] **Step 1: 產生圖示**:做一張 1024×1024 簡潔 logo PNG(深底白色閃電/對話框形,程式繪製即可),跑 `npm run tauri icon <png path>`。
- [ ] **Step 2: bundle 設定**:`"bundle": { "active": true, "targets": ["nsis"], "windows": { "webviewInstallMode": { "type": "downloadBootstrapper" }, "nsis": { "installMode": "currentUser", "languages": ["TradChinese", "English"] } } }`
- [ ] **Step 3: `npm run tauri build`** → Expected: `launcher/src-tauri/target/release/bundle/nsis/*-setup.exe` 產出。
- [ ] **Step 4: 本機安裝煙霧測試**:跑安裝器 → app 出現在系統匣 → Alt+H 正常 → 解除安裝乾淨。
- [ ] **Step 5: Commit** `build: configure NSIS per-user bundle with WebView2 bootstrapper and icons`

---

### Task 14: install.ps1 一行通路 + README 改寫

**Files:** Create: `install.ps1`(repo 根);Modify: `README.md`;Delete: `setup.ps1`、`pull_ollama_cloud_model.py`(以 git rm,功能已由 app 取代)

- [ ] **Step 1: install.ps1**

```powershell
# Free Claude Code 一行安裝:irm <raw-url>/install.ps1 | iex
$ErrorActionPreference = "Stop"
$repo = "jaylooloomi/free-claude-code"
$api = "https://api.github.com/repos/$repo/releases/latest"
Write-Host "[*] 取得最新版本資訊..."
$release = Invoke-RestMethod -Uri $api -UseBasicParsing
$asset = $release.assets | Where-Object { $_.name -like "*-setup.exe" } | Select-Object -First 1
if (-not $asset) { Write-Error "找不到安裝檔,請至 https://github.com/$repo/releases 手動下載"; exit 1 }
$out = Join-Path $env:TEMP $asset.name
Write-Host "[*] 下載 $($asset.name)..."
Invoke-WebRequest -Uri $asset.browser_download_url -OutFile $out -UseBasicParsing
Write-Host "[*] 安裝中(免系統管理員)..."
Start-Process -FilePath $out -ArgumentList "/S" -Wait
Write-Host "[OK] 安裝完成!按 Alt+H 開始使用。"
```
(NSIS 靜默參數為 `/S`;Tauri NSIS bundle 支援。)

- [ ] **Step 2: README.md 全文改寫**:產品介紹(按 Alt+H → 打一句話 → AI 完成)、三種安裝方式(Release 下載 / winget(上架後) / 一行指令)、首次使用流程、設定說明、隱私(零遙測)、開發(tauri dev / cargo test)、授權。
- [ ] **Step 3: 手動驗證 install.ps1 語法**:`powershell -NoProfile -Command "Get-Command -Syntax"` 不適用 — 改為 `pwsh -NoProfile -File install.ps1` 在沒有 release 時應印出「找不到安裝檔」錯誤而非 crash。
- [ ] **Step 4: Commit** `docs: rewrite README for Tauri launcher; add one-line installer; remove legacy scripts`

---

### Task 15: 開發機 E2E

**Files:** Create: `scripts/e2e-local.ps1`

- [ ] **Step 1: 寫腳本**:建 `%TEMP%\fcc-e2e-sandbox` 放 10 個假檔案(txt/jpg/pdf 混合)→ 啟動已安裝的 app(或 `target/release/free-claude-code.exe`)帶 `--run "把這個資料夾裡的檔案依副檔名分類到子資料夾"`,工作目錄先用設定檔指到沙盒資料夾 → 等待終端機程序出現(輪詢 `Get-Process ollama`)→ 人工/腳本確認子資料夾建立。
- [ ] **Step 2: 跑通並記錄結果**(成功標準:終端機開啟、Claude 開始執行、log 檔生成、最終沙盒資料夾被分類)。
- [ ] **Step 3: Commit** `test: add local e2e script`

---

### Task 16: Windows Sandbox 全新電腦 E2E

**Files:** Create: `scripts/e2e-sandbox.wsb`、`scripts/e2e-sandbox-inner.ps1`

- [ ] **Step 1: 檢查沙盒可用**:`Get-WindowsOptionalFeature -Online -FeatureName Containers-DisposableClientVM`(需要時提權啟用;不可用則記錄並以「手動 VM 驗證」代替,不阻塞交付)。
- [ ] **Step 2: e2e-sandbox.wsb**:對映 `launcher/src-tauri/target/release/bundle/nsis`(唯讀)與 `scripts`(唯讀)進沙盒,LogonCommand 跑 inner 腳本。
- [ ] **Step 3: inner 腳本**:靜默裝 setup.exe → 啟動 app `--run "test"` → 預期走 wizard 路徑 → 驗證 Ollama 與 Claude Code 被裝起來(檔案存在 + `ollama --version`)→ 流程停在 signin(預期,沙盒無帳號)→ 輸出檢查報告到對映資料夾。
- [ ] **Step 4: 跑沙盒並收報告**;修正發現的問題(常見:PATH、winget 在沙盒不可用 → 正好驗證直接下載 fallback)。
- [ ] **Step 5: Commit** `test: add Windows Sandbox fresh-machine e2e harness`

---

### Task 17: 最終審查與交付物

- [ ] **Step 1:** `cargo test` + `cargo clippy -- -D warnings` + `npm run check`(svelte-check)+ `npm run tauri build` 全綠。
- [ ] **Step 2:** 對照 spec §1-§11 逐條檢查(requesting-code-review skill:派獨立審查代理看完整 diff)。
- [ ] **Step 3:** 撰寫 `docs/acceptance-checklist.md`(使用者 5 分鐘人工驗收清單:真帳號 signin、真桌面任務、改快捷鍵、背景模式、解除安裝)。
- [ ] **Step 4:** Commit `chore: final review fixes and acceptance checklist`,整理 commit 歷史摘要,通知使用者。

---

## Self-Review 紀錄

- Spec 覆蓋:§4.1→T8/T9;§4.2→T5;§4.3→T7/T10;§4.4→T6;§4.5→T3/T8/T11;§5 錯誤→T12;§7 發佈→T13/T14(winget 上架與簽章為發佈期工作,列 spec 開放事項,不在 v1 代碼範圍);§8 測試→各任務+T15/T16;updater 註:spec §4.5 提及 Tauri updater — **裁定:納入 T13 之後的發佈期工作**,v1 安裝包先不啟用 updater(需簽章金鑰與已發佈的 release 才能端到端驗證;於 acceptance-checklist 與 README 註明)。此為對 spec 的明確縮小,記錄於此。
- Placeholder 掃描:Wizard/Settings 空殼僅存在於 T9 過渡(T10/T11 完整給碼);無 TBD。
- 型別一致性:`Settings`/`StepResult`/`Status`/`LaunchSpec` 欄位在 T3/T5/T6/T7/T8/T9 間已核對一致;`save_settings` 參數名 `newSettings`(camelCase)對應 Rust `new_settings`(Tauri 自動轉換)。
