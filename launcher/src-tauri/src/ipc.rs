use crate::command::SystemRunner;
use crate::http::UreqHttp;
use crate::settings::{Settings, SigninState};
use crate::{bootstrap, catalog, doctor, launcher, logging, settings};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use tauri::{AppHandle, Emitter as _, Manager, State};

/// 佇列中等待執行的任務(記憶體內,不落地)。
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct QueuedTask {
    pub id: u64,
    pub prompt: String,
}

/// 執行中的任務;pid 供 task_stop 以 taskkill 終止背景任務。
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct RunningTask {
    pub id: u64,
    pub prompt: String,
    pub background: bool,
    pub pid: Option<u32>,
}

pub struct AppState {
    pub settings: Mutex<Settings>,
    pub pending_prompt: Mutex<Option<String>>,
    pub catalog_cache: Mutex<Vec<String>>,
    /// FIFO 任務佇列(in-memory;app 重啟即清空)
    pub queue: Mutex<VecDeque<QueuedTask>>,
    pub running: Mutex<Option<RunningTask>>,
    pub next_task_id: AtomicU64,
    /// /api/me 回報的方案("free"/"pro"…);None = 尚未取得
    pub plan: Mutex<Option<String>>,
}

impl AppState {
    pub fn new(settings: Settings) -> Self {
        Self {
            settings: Mutex::new(settings),
            pending_prompt: Mutex::new(None),
            catalog_cache: Mutex::new(Vec::new()),
            queue: Mutex::new(VecDeque::new()),
            running: Mutex::new(None),
            next_task_id: AtomicU64::new(1),
            plan: Mutex::new(None),
        }
    }
}

/// Process-global ensure_server spawn cooldown (shared by doctor checks AND
/// wizard signin/model steps): at most one `ollama serve` spawn per 30s window.
static SERVE_SPAWN_GATE: std::sync::Mutex<Option<std::time::Instant>> = std::sync::Mutex::new(None);

/// Production doctor deps: real runner/http, default claude paths,
/// 200ms × 50 attempts (= wait up to 10s for `ollama serve`).
/// VERSION_CACHE is process-global: the quick_check version gate runs
/// `ollama --version` at most once per app run (tests inject their own).
fn prod_deps<'a>(runner: &'a SystemRunner, http: &'a UreqHttp) -> doctor::Deps<'a> {
    static VERSION_CACHE: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    doctor::Deps {
        runner,
        http,
        claude_paths: doctor::default_claude_paths(),
        serve_poll_ms: 200,
        serve_attempts: 50,
        version_cache: &VERSION_CACHE,
        serve_spawn_gate: &SERVE_SPAWN_GATE,
    }
}

#[derive(serde::Serialize)]
pub struct StatusDto {
    pub state: String,
    pub model: String,
    pub detail: String,
    /// 帳號方案(/api/me 的 plan 欄位);None = 尚未取得
    pub plan: Option<String>,
}

#[tauri::command]
pub fn get_settings(state: State<AppState>) -> Settings {
    state.settings.lock().unwrap().clone()
}

#[tauri::command]
pub async fn save_settings(app: AppHandle, state: State<'_, AppState>, new_settings: Settings) -> Result<(), String> {
    // (a) Read old settings clone before any mutation
    let old_settings = state.settings.lock().unwrap().clone();
    let hotkey_changed = old_settings.hotkey != new_settings.hotkey;

    // (b) If hotkey changed → try register new; on Err → re-register old (best-effort) and return Err
    if hotkey_changed {
        if let Err(e) = crate::register_hotkey(&app, &new_settings.hotkey) {
            let _ = crate::register_hotkey(&app, &old_settings.hotkey);
            return Err(e);
        }
    }

    // (c) Persist to disk with NEW settings — on Err return Err WITHOUT updating in-memory state
    settings::save(&settings::settings_path(), &new_settings).map_err(|e| e.to_string())?;

    // (d) Only now overwrite in-memory state
    *state.settings.lock().unwrap() = new_settings.clone();

    // (e) Sync autostart (always runs when we reach here)
    crate::sync_autostart(&app, new_settings.autostart);
    Ok(())
}

#[tauri::command]
pub async fn get_status(app: AppHandle, state: State<'_, AppState>) -> Result<StatusDto, String> {
    let s = state.settings.lock().unwrap().clone();
    let cat = state.catalog_cache.lock().unwrap().clone();
    let cache_empty = cat.is_empty();
    // 帳號方案:回傳快取值;尚未取得時非阻塞地觸發一次抓取(下次輪詢就有)
    let plan = state.plan.lock().unwrap().clone();
    if plan.is_none() {
        refresh_plan(app.clone());
    }
    let (status, probe) = tauri::async_runtime::spawn_blocking(move || {
        let runner = SystemRunner;
        let http = UreqHttp;
        let status = doctor::quick_check(&prod_deps(&runner, &http));
        // Offline overlay: empty catalog cache means the boot-time refresh failed.
        // Probe the catalog once (3s): Some(Some(models)) = online, Some(None) = offline.
        let probe = if cache_empty {
            use crate::http::Http as _;
            Some(
                http.get(catalog::CATALOG_URL, std::time::Duration::from_secs(3))
                    .ok()
                    .map(|json| catalog::parse_cloud_models(&json).unwrap_or_default()),
            )
        } else {
            None
        };
        (status, probe)
    })
    .await
    .map_err(|e| e.to_string())?;
    let mut cat = cat;
    if let Some(fetch) = probe {
        match fetch {
            None => {
                // Cloud catalog unreachable → cloud models cannot work. Be honest.
                let (model, _) = catalog::choose_model(&s.model, &cat);
                return Ok(StatusDto {
                    state: "offline".into(),
                    model,
                    detail: "離線 — 雲端模型需要網路連線".into(),
                    plan,
                });
            }
            Some(models) => {
                if !models.is_empty() {
                    *state.catalog_cache.lock().unwrap() = models.clone();
                    cat = models;
                }
            }
        }
    }
    let (model, _) = catalog::choose_model(&s.model, &cat);
    Ok(match status {
        doctor::Status::Ready => StatusDto { state: "ready".into(), model, detail: String::new(), plan },
        doctor::Status::NeedsSetup { .. } => StatusDto {
            state: "needs_setup".into(),
            model,
            detail: "首次使用將自動安裝必要元件".into(),
            plan,
        },
        doctor::Status::Degraded { reason } => StatusDto { state: "degraded".into(), model, detail: reason, plan },
    })
}

#[tauri::command]
pub fn get_history(state: State<AppState>) -> Vec<String> {
    state.settings.lock().unwrap().history.clone()
}

/// 回傳 "launched" | "queued" | "wizard";Err(中文訊息) 顯示在面板。
#[tauri::command]
pub async fn submit_prompt(app: AppHandle, state: State<'_, AppState>, prompt: String) -> Result<String, String> {
    let prompt = prompt.trim().to_string();
    if prompt.is_empty() {
        return Err("請輸入需求".into());
    }
    let signin_no;
    {
        let mut s = state.settings.lock().unwrap();
        s.push_history(&prompt);
        let _ = settings::save(&settings::settings_path(), &s);
        signin_no = s.signin_state == SigninState::No;
    }
    // 登入已失效 → 重新走精靈(auth re-onboarding)
    if signin_no {
        *state.pending_prompt.lock().unwrap() = Some(prompt);
        show_window(&app, "wizard");
        hide_window(&app, "palette");
        return Ok("wizard".into());
    }
    let status = tauri::async_runtime::spawn_blocking(move || {
        let runner = SystemRunner;
        let http = UreqHttp;
        doctor::quick_check(&prod_deps(&runner, &http))
    })
    .await
    .map_err(|e| e.to_string())?;
    match status {
        doctor::Status::NeedsSetup { .. } => {
            *state.pending_prompt.lock().unwrap() = Some(prompt);
            show_window(&app, "wizard");
            hide_window(&app, "palette");
            Ok("wizard".into())
        }
        doctor::Status::Degraded { reason } => Err(reason),
        doctor::Status::Ready => {
            *state.pending_prompt.lock().unwrap() = None;
            let task = new_task(&state, prompt);
            let outcome = launch_or_enqueue(&app, task)?;
            if outcome == "launched" {
                hide_window(&app, "palette");
            }
            Ok(outcome.into())
        }
    }
}

/// 配發單調遞增 id 的新任務。
fn new_task(state: &AppState, prompt: String) -> QueuedTask {
    QueuedTask { id: state.next_task_id.fetch_add(1, Ordering::Relaxed), prompt }
}

/// 純佇列判斷:已有任務執行中 → 入列並回傳 None;否則把任務還給呼叫者啟動。
/// (拆成純函式以便單元測試;launch_or_enqueue 負責 emit 與實際啟動)
fn try_enqueue(state: &AppState, task: QueuedTask) -> Option<QueuedTask> {
    if state.running.lock().unwrap().is_some() {
        state.queue.lock().unwrap().push_back(task);
        None
    } else {
        Some(task)
    }
}

/// Ready 狀態的統一入口:執行中 → 排入佇列("queued");否則立即啟動("launched")。
fn launch_or_enqueue(app: &AppHandle, task: QueuedTask) -> Result<&'static str, String> {
    let state = app.state::<AppState>();
    match try_enqueue(&state, task) {
        None => {
            emit_queue_changed(app);
            Ok("queued")
        }
        Some(task) => {
            do_launch(app, task)?;
            Ok("launched")
        }
    }
}

/// 純佇列變更:清掉 running、取出佇列最前面的任務(FIFO)。
fn take_next(state: &AppState) -> Option<QueuedTask> {
    *state.running.lock().unwrap() = None;
    state.queue.lock().unwrap().pop_front()
}

/// 通知用的需求預覽(前 20 個字,按字元數而非位元組,CJK 安全)。
fn prompt_preview(prompt: &str) -> String {
    prompt.chars().take(20).collect()
}

/// 任務結束後的佇列接續:清掉 running、取出下一個任務並啟動。
/// 啟動失敗時通知並繼續嘗試下一個,避免佇列卡死。
pub fn start_or_queue_next(app: &AppHandle) {
    let state = app.state::<AppState>();
    let next = take_next(&state);
    emit_queue_changed(app);
    if let Some(task) = next {
        crate::notify(app, &format!("開始執行:{}…", prompt_preview(&task.prompt)));
        if let Err(e) = do_launch(app, task) {
            crate::notify(app, &e);
            start_or_queue_next(app);
        }
    }
}

fn emit_queue_changed(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("palette") {
        let _ = w.emit("queue-changed", ());
    }
}

pub fn do_launch(app: &AppHandle, task: QueuedTask) -> Result<(), String> {
    let state = app.state::<AppState>();
    let s = state.settings.lock().unwrap().clone();
    let cat = state.catalog_cache.lock().unwrap().clone();
    let (model, notice) = catalog::choose_model(&s.model, &cat);
    if let Some(n) = notice {
        crate::notify(app, &n);
    }
    let spec = launcher::build_launch_spec(&task.prompt, &s, &model);
    let is_background = spec.background;
    let dir = logging::logs_dir();
    logging::rotate(&dir, 30);
    let log = logging::new_run_log(&dir).map_err(|e| format!("無法建立記錄檔:{e}"))?;
    let app2 = app.clone();
    // 兩種模式 waiter 都會呼叫(spawn v1.1 契約):先處理通知,再接續佇列。
    let on_done: launcher::OnDone = Box::new(move |code, log_path, elapsed| {
        handle_task_exit(&app2, code, &log_path, elapsed, is_background, &model);
        start_or_queue_next(&app2);
    });
    {
        // 持鎖橫跨 spawn → 設 running:waiter 的 start_or_queue_next 第一步就鎖
        // running,因此「極速結束」也不會在 running 寫入前被清掉。
        let mut running = state.running.lock().unwrap();
        let pid = launcher::spawn(&spec, log, Some(on_done)).map_err(|e| format!("啟動失敗:{e}"))?;
        *running = Some(RunningTask { id: task.id, prompt: task.prompt, background: is_background, pid: Some(pid) });
    }
    // 成功啟動過 → 視為已登入(auth 失敗時 runtime 會改回 No)
    {
        let mut st = state.settings.lock().unwrap();
        if st.signin_state == SigninState::Unknown {
            st.signin_state = SigninState::Yes;
            let _ = settings::save(&settings::settings_path(), &st);
        }
    }
    emit_queue_changed(app);
    Ok(())
}

/// 任務結束通知決策(原本散在 launcher.rs 的 fast-fail 判斷移到這裡):
/// - code 0:背景 → 「任務完成」;前景 → 靜默
/// - 非 0 前景且 30 秒內結束 → fast-fail 訊息(前景無記錄檔可分類)
/// - 非 0 前景且超過 30 秒 → 靜默(多半是使用者自行關閉終端機,沿用 v1 行為)
/// - 非 0 其餘(背景)→ classify_failure 流程
fn handle_task_exit(
    app: &AppHandle,
    code: i32,
    log_path: &std::path::Path,
    elapsed: std::time::Duration,
    is_background: bool,
    model: &str,
) {
    if code == 0 {
        if is_background {
            crate::notify(app, "任務完成");
        }
        return;
    }
    let fast = elapsed < std::time::Duration::from_secs(30);
    if !is_background && !fast {
        return;
    }
    let tail = read_log_tail(log_path);
    if !is_background && tail.is_empty() {
        // 前景 fast-fail 且無記錄輸出 — 可能是訂閱/額度/登入問題
        let msg = format!("啟動後立即失敗 (exit {code}),可能是模型需訂閱、額度用盡或登入失效");
        crate::notify(app, &msg);
        return;
    }
    match launcher::classify_failure(&tail) {
        launcher::FailureKind::Subscription => {
            // tier learning:記住這個模型要訂閱,模型選單據此標示
            let state = app.state::<AppState>();
            {
                let mut st = state.settings.lock().unwrap();
                if !st.known_subscription_models.iter().any(|m| m == model) {
                    st.known_subscription_models.push(model.to_string());
                    let _ = settings::save(&settings::settings_path(), &st);
                }
            }
            crate::notify(app, "此模型需要付費訂閱 — 請到設定改用免費模型(如 minimax-m2.5:cloud)");
        }
        launcher::FailureKind::Quota => {
            crate::notify(app, "免費額度已用完,稍後重置(限制綁帳號,換模型無效)");
        }
        launcher::FailureKind::Auth => {
            // Re-lock sign-in state so next submit_prompt triggers the wizard
            let state = app.state::<AppState>();
            {
                let mut st = state.settings.lock().unwrap();
                st.signin_state = SigninState::No;
                let _ = settings::save(&settings::settings_path(), &st);
            }
            crate::notify(app, "需要重新登入 ollama.com,下次啟動會自動引導");
        }
        launcher::FailureKind::Other => {
            let msg = if code == -1 {
                format!("任務異常結束(原因未知),記錄:{}", log_path.display())
            } else {
                format!("任務失敗 (exit {code}),記錄:{}", log_path.display())
            };
            crate::notify(app, &msg);
        }
    }
}

/// Read last 4 KB of log for classification.
fn read_log_tail(log_path: &std::path::Path) -> String {
    std::fs::read(log_path)
        .map(|bytes| {
            let start = bytes.len().saturating_sub(4096);
            String::from_utf8_lossy(&bytes[start..]).into_owned()
        })
        .unwrap_or_default()
}

#[derive(serde::Serialize)]
pub struct WizardPlan {
    pub steps: Vec<String>,
}

#[tauri::command]
pub async fn wizard_plan(state: State<'_, AppState>) -> Result<WizardPlan, String> {
    let (model, signed) = {
        let s = state.settings.lock().unwrap();
        (s.model.clone(), s.signin_state == SigninState::Yes)
    };
    let status = tauri::async_runtime::spawn_blocking(move || {
        let runner = SystemRunner;
        let http = UreqHttp;
        doctor::full_check(&prod_deps(&runner, &http), &model)
    })
    .await
    .map_err(|e| e.to_string())?;
    let mut steps: Vec<String> = Vec::new();
    if let doctor::Status::NeedsSetup { missing } = status {
        for c in missing {
            match c {
                doctor::Component::Ollama => steps.push("ollama".into()),
                doctor::Component::OllamaUpgrade => steps.push("ollama_upgrade".into()),
                doctor::Component::ClaudeCode => steps.push("claude".into()),
                // 由最後無條件附加的 "model" 步驟涵蓋(去重)
                doctor::Component::Model => {}
            }
        }
    }
    if !signed {
        steps.push("signin".into());
    }
    steps.push("model".into());
    Ok(WizardPlan { steps })
}

#[tauri::command]
pub async fn wizard_run(state: State<'_, AppState>, step: String) -> Result<bootstrap::StepResult, String> {
    let model = state.settings.lock().unwrap().model.clone();
    let step2 = step.clone();
    let res = tauri::async_runtime::spawn_blocking(move || {
        let runner = SystemRunner;
        let http = UreqHttp;
        match step2.as_str() {
            "ollama" | "ollama_upgrade" => bootstrap::install_ollama(&runner, &http, std::env::temp_dir()),
            "claude" => bootstrap::install_claude(&runner),
            // signin/model talk to the local daemon — make sure it is up first
            // (200ms × 50 = wait up to 10s, same as prod_deps).
            "signin" | "model" => {
                if !doctor::ensure_server(&runner, &http, 200, 50, &SERVE_SPAWN_GATE) {
                    bootstrap::StepResult { ok: false, detail: "Ollama 服務尚未就緒,請重試".into() }
                } else if step2 == "signin" {
                    bootstrap::signin(&runner)
                } else {
                    bootstrap::register_model(&runner, &model)
                }
            }
            other => bootstrap::StepResult { ok: false, detail: format!("未知步驟 {other}") },
        }
    })
    .await
    .map_err(|e| e.to_string())?;
    // 安裝精靈記錄(best-effort):%APPDATA%\free-claude-code\logs\wizard.log
    {
        let dir = logging::logs_dir();
        let _ = std::fs::create_dir_all(&dir);
        let line = format!(
            "[{}] step={} ok={} detail={}\n",
            chrono::Local::now().format("%Y-%m-%d %H:%M:%S"),
            step,
            res.ok,
            res.detail
        );
        let _ = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(dir.join("wizard.log"))
            .and_then(|mut f| std::io::Write::write_all(&mut f, line.as_bytes()));
    }
    if step == "signin" && res.ok {
        let mut s = state.settings.lock().unwrap();
        s.signin_state = SigninState::Yes;
        let _ = settings::save(&settings::settings_path(), &s);
    }
    Ok(res)
}

#[tauri::command]
pub async fn wizard_done(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    // 先嘗試啟動暫存需求;失敗時放回暫存並保留精靈視窗,讓使用者可重試
    let pending = state.pending_prompt.lock().unwrap().take();
    if let Some(p) = pending {
        let task = new_task(&state, p.clone());
        if let Err(e) = launch_or_enqueue(&app, task) {
            *state.pending_prompt.lock().unwrap() = Some(p);
            return Err(e);
        }
    }
    hide_window(&app, "wizard");
    Ok(())
}

#[tauri::command]
pub fn list_cloud_models(state: State<AppState>) -> Vec<String> {
    state.catalog_cache.lock().unwrap().clone()
}

// ---------- v1.1: task queue ----------

#[derive(serde::Serialize)]
pub struct QueueDto {
    pub running: Option<RunningTask>,
    pub queued: Vec<QueuedTask>,
}

#[tauri::command]
pub fn queue_list(state: State<AppState>) -> QueueDto {
    QueueDto {
        running: state.running.lock().unwrap().clone(),
        queued: state.queue.lock().unwrap().iter().cloned().collect(),
    }
}

/// 純佇列變更:移除指定 id,回傳是否有移除。
fn cancel_in_queue(state: &AppState, id: u64) -> bool {
    let mut q = state.queue.lock().unwrap();
    let before = q.len();
    q.retain(|t| t.id != id);
    q.len() != before
}

#[tauri::command]
pub fn queue_cancel(state: State<AppState>, app: AppHandle, id: u64) -> Result<(), String> {
    if !cancel_in_queue(&state, id) {
        return Err("佇列中找不到該任務".into());
    }
    emit_queue_changed(&app);
    Ok(())
}

/// 純決策:目前 running 能否被停止?能 → 回傳要 taskkill 的 pid。
fn stop_decision(running: &Option<RunningTask>) -> Result<u32, String> {
    match running {
        None => Err("目前沒有執行中的任務".into()),
        Some(rt) if !rt.background => Err("前景任務請直接關閉其終端機視窗".into()),
        Some(rt) => rt.pid.ok_or_else(|| "無法取得任務的處理程序 ID".into()),
    }
}

/// 停止背景任務:taskkill 整個 process tree;不在這裡清 running —
/// waiter 執行緒的 on_done 會自然觸發並接續佇列。
#[tauri::command]
pub fn task_stop(state: State<AppState>, _app: AppHandle) -> Result<(), String> {
    let pid = stop_decision(&state.running.lock().unwrap())?;
    use crate::command::Runner as _;
    SystemRunner
        .spawn_detached("taskkill", &["/PID", &pid.to_string(), "/T", "/F"])
        .map_err(|e| e.to_string())
}

// ---------- v1.1: model selection + tier learning ----------

#[derive(serde::Serialize)]
pub struct ModelEntry {
    pub name: String,
    pub tier: String,
}

/// "free":實證免費名單;"subscription":執行時學到要訂閱;其餘 "unknown"。
fn compute_tier(name: &str, known_subscription: &[String]) -> &'static str {
    if catalog::VERIFIED_FREE.contains(&name) {
        "free"
    } else if known_subscription.iter().any(|m| m == name) {
        "subscription"
    } else {
        "unknown"
    }
}

#[tauri::command]
pub fn list_models_ui(state: State<AppState>) -> Vec<ModelEntry> {
    let (current, known) = {
        let s = state.settings.lock().unwrap();
        (s.model.clone(), s.known_subscription_models.clone())
    };
    let cat = state.catalog_cache.lock().unwrap().clone();
    // 離線/尚未抓到目錄時至少回傳目前設定的模型
    let names = if cat.is_empty() { vec![current] } else { cat };
    names
        .into_iter()
        .map(|name| {
            let tier = compute_tier(&name, &known).to_string();
            ModelEntry { name, tier }
        })
        .collect()
}

#[tauri::command]
pub async fn set_model(_app: AppHandle, state: State<'_, AppState>, name: String) -> Result<(), String> {
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err("模型名稱不可為空".into());
    }
    let mut s = state.settings.lock().unwrap();
    s.model = name;
    settings::save(&settings::settings_path(), &s).map_err(|e| e.to_string())
}

/// webview 不得開任意網址 — 僅允許白名單(完全比對)。
fn url_allowed(url: &str) -> bool {
    const ALLOWED: [&str; 2] = ["https://ollama.com/settings", "https://ollama.com/upgrade"];
    ALLOWED.contains(&url)
}

#[tauri::command]
pub fn open_url(url: String) -> Result<(), String> {
    if !url_allowed(&url) {
        return Err("不允許開啟此網址".into());
    }
    use crate::command::Runner as _;
    SystemRunner.spawn_detached("explorer", &[&url]).map_err(|e| e.to_string())
}

// ---------- v1.1: acrylic effect state ----------

/// 前端據此決定玻璃(fx-glass)或純色(fx-solid)樣式。
#[tauri::command]
pub fn effects_applied() -> bool {
    crate::fx::effects_applied()
}

// ---------- v1.1: account plan (/api/me) ----------

pub const API_ME_URL: &str = "http://127.0.0.1:11434/api/me";

/// 從 /api/me 的 JSON 取出 plan 欄位;缺欄位/非字串/壞 JSON → None。
fn parse_plan(json: &str) -> Option<String> {
    serde_json::from_str::<serde_json::Value>(json)
        .ok()?
        .get("plan")?
        .as_str()
        .map(str::to_string)
}

/// 以注入的 Http 抓方案(可測);網路錯誤或缺欄位 → None。
fn fetch_plan(http: &dyn crate::http::Http) -> Option<String> {
    let body = http.post(API_ME_URL, "{}", std::time::Duration::from_secs(5)).ok()?;
    parse_plan(&body)
}

/// 非阻塞更新帳號方案快取;啟動時呼叫一次,get_status 在快取為空時也會補觸發。
pub fn refresh_plan(app: AppHandle) {
    tauri::async_runtime::spawn_blocking(move || {
        let plan = fetch_plan(&UreqHttp);
        if plan.is_some() {
            let state = app.state::<AppState>();
            *state.plan.lock().unwrap() = plan;
        }
    });
}

// ---------- v1.1: voice input (Win+H) ----------

/// 確保輸入面板可見且取得焦點後,送出 Win+H 啟動 Windows 語音輸入。
/// 50ms 延遲讓焦點切換先完成,語音輸入才會落在面板的輸入框。
#[tauri::command]
pub async fn start_voice_input(app: AppHandle) -> Result<(), String> {
    let Some(w) = app.get_webview_window("palette") else {
        return Err("找不到輸入面板視窗".into());
    };
    let _ = w.show();
    let _ = w.set_focus();
    std::thread::sleep(std::time::Duration::from_millis(50));
    crate::voice::trigger_voice_typing();
    Ok(())
}

#[tauri::command]
pub fn open_logs() -> Result<(), String> {
    let dir = logging::logs_dir();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    use crate::command::Runner as _;
    SystemRunner
        .spawn_detached("explorer", &[&dir.to_string_lossy()])
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn hide_palette(app: AppHandle) {
    hide_window(&app, "palette");
}

#[tauri::command]
pub fn open_settings_window(app: AppHandle) {
    show_window(&app, "settings");
}

pub fn show_window(app: &AppHandle, label: &str) {
    if let Some(w) = app.get_webview_window(label) {
        let _ = w.show();
        let _ = w.set_focus();
        // 通知前端「視窗真正被顯示」— 視窗啟動時皆為隱藏,前端據此才初始化
        let _ = w.emit(&format!("{label}-shown"), ());
    }
}

pub fn hide_window(app: &AppHandle, label: &str) {
    if let Some(w) = app.get_webview_window(label) {
        let _ = w.hide();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::MockHttp;

    // NOTE 測試涵蓋範圍:tauri::command 函式需要 AppHandle/State(無 tauri "test"
    // feature 可用),因此命令本體與事件發送(queue-changed)、do_launch 的
    // RunningTask 記錄、on_done 通知與佇列接續(start_or_queue_next 經由真實
    // waiter 執行緒觸發)只能由 E2E 驗證。單元測試鎖定其中的純邏輯:
    // 佇列變更(try_enqueue/cancel_in_queue/take_next)、停止決策(stop_decision)、
    // tier 計算、URL 白名單、/api/me 解析。

    fn state() -> AppState {
        AppState::new(Settings::default())
    }

    fn enqueue_prompt(st: &AppState, prompt: &str) -> QueuedTask {
        let task = new_task(st, prompt.into());
        assert!(try_enqueue(st, task.clone()).is_none(), "running 中應入列");
        task
    }

    #[test]
    fn task_ids_are_unique_and_increasing() {
        let st = state();
        let a = new_task(&st, "a".into());
        let b = new_task(&st, "b".into());
        assert!(b.id > a.id);
    }

    #[test]
    fn try_enqueue_launches_directly_when_idle() {
        let st = state();
        let task = new_task(&st, "hi".into());
        // 沒有 running → 任務交還呼叫者啟動,佇列保持空
        let returned = try_enqueue(&st, task.clone());
        assert_eq!(returned, Some(task));
        assert!(st.queue.lock().unwrap().is_empty());
    }

    #[test]
    fn try_enqueue_queues_fifo_while_running() {
        let st = state();
        *st.running.lock().unwrap() =
            Some(RunningTask { id: 99, prompt: "busy".into(), background: true, pid: Some(1234) });
        let a = enqueue_prompt(&st, "first");
        let b = enqueue_prompt(&st, "second");
        let c = enqueue_prompt(&st, "third");
        let q: Vec<u64> = st.queue.lock().unwrap().iter().map(|t| t.id).collect();
        assert_eq!(q, vec![a.id, b.id, c.id], "佇列必須維持 FIFO 順序");
    }

    #[test]
    fn cancel_removes_only_the_requested_id() {
        let st = state();
        *st.running.lock().unwrap() =
            Some(RunningTask { id: 99, prompt: "busy".into(), background: true, pid: None });
        let a = enqueue_prompt(&st, "first");
        let b = enqueue_prompt(&st, "second");
        let c = enqueue_prompt(&st, "third");
        assert!(cancel_in_queue(&st, b.id));
        let q: Vec<u64> = st.queue.lock().unwrap().iter().map(|t| t.id).collect();
        assert_eq!(q, vec![a.id, c.id]);
        assert!(!cancel_in_queue(&st, b.id), "重複取消同 id 應回報找不到");
        assert!(!cancel_in_queue(&st, 424242), "不存在的 id 應回報找不到");
    }

    #[test]
    fn take_next_clears_running_and_pops_fifo() {
        let st = state();
        *st.running.lock().unwrap() =
            Some(RunningTask { id: 99, prompt: "busy".into(), background: true, pid: Some(1) });
        let a = enqueue_prompt(&st, "first");
        let b = enqueue_prompt(&st, "second");
        let next = take_next(&st).unwrap();
        assert_eq!(next.id, a.id);
        assert!(st.running.lock().unwrap().is_none(), "take_next 必須清掉 running");
        assert_eq!(take_next(&st).unwrap().id, b.id);
        assert!(take_next(&st).is_none(), "佇列清空後應回 None");
    }

    #[test]
    fn stop_decision_rules() {
        assert!(stop_decision(&None).is_err(), "沒有執行中任務不能停止");
        let fg = Some(RunningTask { id: 1, prompt: "p".into(), background: false, pid: Some(10) });
        assert_eq!(stop_decision(&fg).unwrap_err(), "前景任務請直接關閉其終端機視窗");
        let bg = Some(RunningTask { id: 2, prompt: "p".into(), background: true, pid: Some(4321) });
        assert_eq!(stop_decision(&bg).unwrap(), 4321);
        let bg_no_pid = Some(RunningTask { id: 3, prompt: "p".into(), background: true, pid: None });
        assert!(stop_decision(&bg_no_pid).is_err());
    }

    #[test]
    fn prompt_preview_truncates_to_20_chars_cjk_safe() {
        assert_eq!(prompt_preview("short"), "short");
        let long = "整理桌面上的所有檔案並依照副檔名分類到資料夾中"; // 23 chars
        let p = prompt_preview(long);
        assert_eq!(p.chars().count(), 20);
        assert!(long.starts_with(&p));
    }

    #[test]
    fn compute_tier_free_subscription_unknown() {
        let known = vec!["minimax-m2.7:cloud".to_string()];
        assert_eq!(compute_tier("minimax-m2.5:cloud", &known), "free");
        assert_eq!(compute_tier("qwen3-coder-next:cloud", &known), "free");
        assert_eq!(compute_tier("glm-4.7:cloud", &known), "free");
        assert_eq!(compute_tier("minimax-m2.7:cloud", &known), "subscription");
        assert_eq!(compute_tier("gpt-oss:120b-cloud", &known), "unknown");
        // 同時在兩邊時以實證免費為準
        let conflict = vec!["minimax-m2.5:cloud".to_string()];
        assert_eq!(compute_tier("minimax-m2.5:cloud", &conflict), "free");
    }

    #[test]
    fn url_whitelist_is_exact_match_only() {
        assert!(url_allowed("https://ollama.com/settings"));
        assert!(url_allowed("https://ollama.com/upgrade"));
        assert!(!url_allowed("https://ollama.com/upgrade?x=1"));
        assert!(!url_allowed("https://ollama.com/"));
        assert!(!url_allowed("https://evil.example.com/https://ollama.com/upgrade"));
        assert!(!url_allowed("http://ollama.com/upgrade"));
        assert!(!url_allowed(""));
    }

    #[test]
    fn parse_plan_from_realistic_api_me_json() {
        let json = r#"{"id":"x","email":"e","name":"n","plan":"free"}"#;
        assert_eq!(parse_plan(json), Some("free".to_string()));
        assert_eq!(parse_plan(r#"{"plan":"pro"}"#), Some("pro".to_string()));
    }

    #[test]
    fn parse_plan_tolerates_missing_or_malformed() {
        assert_eq!(parse_plan(r#"{"id":"x","email":"e"}"#), None, "缺 plan 欄位 → None");
        assert_eq!(parse_plan(r#"{"plan":123}"#), None, "plan 非字串 → None");
        assert_eq!(parse_plan("not json"), None);
        assert_eq!(parse_plan(""), None);
    }

    #[test]
    fn fetch_plan_posts_json_body_to_api_me() {
        let http = MockHttp::default()
            .on_post(API_ME_URL, Ok(r#"{"id":"x","email":"e","name":"n","plan":"free"}"#));
        assert_eq!(fetch_plan(&http), Some("free".to_string()));
        let posts = http.posts.lock().unwrap();
        assert_eq!(posts.as_slice(), &[(API_ME_URL.to_string(), "{}".to_string())]);
    }

    #[test]
    fn fetch_plan_returns_none_on_network_error() {
        let http = MockHttp::default(); // unmocked → Err
        assert_eq!(fetch_plan(&http), None);
    }
}
