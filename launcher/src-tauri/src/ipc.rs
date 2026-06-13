use crate::command::SystemRunner;
use crate::http::UreqHttp;
use crate::settings::{Settings, SigninState};
use crate::{bootstrap, catalog, doctor, launcher, logging, settings};
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use tauri::{AppHandle, Emitter as _, Manager, State};

/// 佇列中等待執行的任務(記憶體內,不落地)。
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct QueuedTask {
    pub id: u64,
    pub prompt: String,
}

/// 執行中的任務;pid 供 task_stop 以 taskkill 終止背景任務。
/// stopping = 使用者已按下停止(task_stop)— 結束時不得當作失敗通知。
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct RunningTask {
    pub id: u64,
    pub prompt: String,
    pub background: bool,
    pub pid: Option<u32>,
    pub stopping: bool,
}

/// 佇列狀態機:running + queued 必須在同一把鎖下變更,
/// 否則「判斷是否執行中」與「入列/啟動」之間的空窗會讓
/// double-submit 同時跑兩個任務、或讓佇列任務被遺落(TOCTOU)。
#[derive(Default)]
pub struct QueueState {
    pub running: Option<RunningTask>,
    pub queued: VecDeque<QueuedTask>,
}

pub struct AppState {
    pub settings: Mutex<Settings>,
    pub pending_prompt: Mutex<Option<String>>,
    pub catalog_cache: Mutex<Vec<String>>,
    /// FIFO 任務佇列+執行中任務(單一鎖 = 單一事實來源;in-memory,app 重啟即清空)
    pub queue: Mutex<QueueState>,
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
            queue: Mutex::new(QueueState::default()),
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

/// server-side overlay:以「目前記憶體設定」為底,只接受 UI 可編輯的欄位
/// (hotkey/model/cautious/background/working_dir/autostart/locale)。
/// history/signin_state/known_*_models 由後端在設定視窗開啟期間持續更新
/// (submit_prompt、tier learning、被動學習、掃描、auth re-lock),不可被前端的舊快照蓋掉。
fn overlay_ui_fields(current: &Settings, incoming: &Settings) -> Settings {
    Settings {
        hotkey: incoming.hotkey.clone(),
        model: incoming.model.clone(),
        cautious_mode: incoming.cautious_mode,
        background_mode: incoming.background_mode,
        working_dir: incoming.working_dir.clone(),
        autostart: incoming.autostart,
        locale: incoming.locale.clone(),
        system_prompt: incoming.system_prompt.clone(),
        history: current.history.clone(),
        signin_state: current.signin_state.clone(),
        known_subscription_models: current.known_subscription_models.clone(),
        known_free_models: current.known_free_models.clone(),
        known_broken_models: current.known_broken_models.clone(),
    }
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

    // (c)+(d) 同一把 settings 鎖內:以當下記憶體值合併(overlay)→ 先落地 →
    // 再提交記憶體;落地失敗回 Err 且不動記憶體(沿用既有順序)
    {
        let mut s = state.settings.lock().unwrap();
        let merged = overlay_ui_fields(&s, &new_settings);
        settings::save(&settings::settings_path(), &merged).map_err(|e| e.to_string())?;
        crate::i18n::set_locale(&merged.locale);
        *s = merged;
    }

    // (e) Sync autostart (always runs when we reach here)
    crate::sync_autostart(&app, new_settings.autostart);
    Ok(())
}

#[tauri::command]
pub async fn get_status(app: AppHandle, state: State<'_, AppState>) -> Result<StatusDto, String> {
    let s = state.settings.lock().unwrap().clone();
    crate::i18n::set_locale(&s.locale);

    // claude(Anthropic 帳號)路徑:完全不碰 Ollama,只看 claude.exe 是否安裝。
    if s.model == catalog::CLAUDE_MODEL {
        let claude_status = tauri::async_runtime::spawn_blocking(|| {
            let runner = SystemRunner;
            let http = UreqHttp;
            (doctor::claude_check(&prod_deps(&runner, &http)), doctor::claude_authed())
        })
        .await
        .map_err(|e| e.to_string())?;
        let (state_str, detail) = match claude_status {
            (doctor::Status::Ready, true) => ("ready", String::new()),
            (doctor::Status::Ready, false) => ("ready", crate::i18n::claude_not_logged_in()),
            (doctor::Status::Degraded { reason }, _) => ("degraded", reason),
            _ => ("degraded", crate::i18n::claude_status_unknown()),
        };
        return Ok(StatusDto {
            state: state_str.into(),
            model: catalog::CLAUDE_MODEL.into(),
            detail,
            plan: None,
        });
    }

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
                    detail: crate::i18n::offline_cloud_needs_network(),
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
    // 顯示「實際會用」的模型;若選中的不可用,switch_notice 提示已自動改用免費模型
    let (model, switch_notice) = resolve_usable_model(&s, &cat);
    Ok(match status {
        doctor::Status::Ready => StatusDto {
            state: "ready".into(),
            model,
            detail: switch_notice.unwrap_or_default(),
            plan,
        },
        doctor::Status::NeedsSetup { .. } => StatusDto {
            state: "needs_setup".into(),
            model,
            detail: crate::i18n::first_run_will_install(),
            plan,
        },
        doctor::Status::Degraded { reason } => StatusDto { state: "degraded".into(), model, detail: reason, plan },
    })
}

#[tauri::command]
pub fn get_history(state: State<AppState>) -> Vec<String> {
    state.settings.lock().unwrap().history.clone()
}

/// 只接受已知影像副檔名,其他一律當 png(避免任意副檔名寫進暫存路徑)。
fn sanitize_image_ext(ext: &str) -> String {
    match ext.to_lowercase().as_str() {
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" => ext.to_lowercase(),
        _ => "png".into(),
    }
}

/// 把貼上/拖入的圖片位元組存成暫存檔,回傳路徑供帶入 prompt(Claude Code 讀檔看圖)。
#[tauri::command]
pub fn save_pasted_image(data: Vec<u8>, ext: String) -> Result<String, String> {
    use std::time::{SystemTime, UNIX_EPOCH};
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let dir = std::env::temp_dir().join("free-claude-code-paste");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join(format!("paste-{stamp}.{}", sanitize_image_ext(&ext)));
    std::fs::write(&path, &data).map_err(|e| e.to_string())?;
    Ok(path.to_string_lossy().into_owned())
}

/// 回傳 "launched" | "queued" | "wizard";Err(中文訊息) 顯示在面板。
#[tauri::command]
pub async fn submit_prompt(app: AppHandle, state: State<'_, AppState>, prompt: String) -> Result<String, String> {
    let prompt = prompt.trim().to_string();
    let (signin_no, is_claude);
    {
        let mut s = state.settings.lock().unwrap();
        crate::i18n::set_locale(&s.locale);
        if prompt.is_empty() {
            return Err(crate::i18n::empty_prompt());
        }
        s.push_history(&prompt);
        let _ = settings::save(&settings::settings_path(), &s);
        signin_no = s.signin_state == SigninState::No;
        is_claude = s.model == catalog::CLAUDE_MODEL;
    }
    // claude(Anthropic 帳號)路徑:跳過 Ollama signin/wizard,只確認 claude.exe 在。
    if is_claude {
        let status = tauri::async_runtime::spawn_blocking(|| {
            let runner = SystemRunner;
            let http = UreqHttp;
            doctor::claude_check(&prod_deps(&runner, &http))
        })
        .await
        .map_err(|e| e.to_string())?;
        return match status {
            doctor::Status::Degraded { reason } => Err(reason),
            _ => {
                *state.pending_prompt.lock().unwrap() = None;
                let task = new_task(&state, prompt);
                let outcome = launch_or_enqueue(&app, task)?;
                if outcome == "launched" {
                    hide_window(&app, "palette");
                }
                Ok(outcome.into())
            }
        };
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

/// 預約紀錄:決策當下就佔住 running 槽位,pid/background 由 do_launch
/// 在 spawn 成功後(同一把 queue 鎖內)補上。
fn reservation(task: &QueuedTask) -> RunningTask {
    RunningTask { id: task.id, prompt: task.prompt.clone(), background: false, pid: None, stopping: false }
}

/// 原子決策+預約(C1):單一鎖內完成「判斷 idle」與「佔住 running 槽位」。
/// idle → 立刻寫入預約並把任務交還呼叫者啟動;否則入列回傳 None。
/// 預約存在後,任何緊接著的決策都只會入列 — double-submit 不可能並行執行。
fn reserve_or_enqueue(state: &AppState, task: QueuedTask) -> Option<QueuedTask> {
    let mut q = state.queue.lock().unwrap();
    if q.running.is_none() {
        q.running = Some(reservation(&task));
        Some(task)
    } else {
        q.queued.push_back(task);
        None
    }
}

/// 共用佇列接續(呼叫端必須已持有 queue 鎖):清掉 running、彈出佇列頭,
/// 並在同一鎖內為彈出的任務建立預約 — 解鎖後的任何 enqueue 決策都會看到
/// running 已被佔用,不會與彈出的任務並行。
fn pop_and_reserve_next(q: &mut QueueState) -> Option<QueuedTask> {
    q.running = None;
    let next = q.queued.pop_front();
    if let Some(task) = &next {
        q.running = Some(reservation(task));
    }
    next
}

/// Ready 狀態的統一入口:執行中 → 排入佇列("queued");否則立即啟動("launched")。
fn launch_or_enqueue(app: &AppHandle, task: QueuedTask) -> Result<&'static str, String> {
    let state = app.state::<AppState>();
    match reserve_or_enqueue(&state, task) {
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

/// 佇列接續(任務結束時):單一鎖內清掉 running 並原子彈出+預約下一個任務。
fn take_next(state: &AppState) -> Option<QueuedTask> {
    pop_and_reserve_next(&mut state.queue.lock().unwrap())
}

/// 通知用的需求預覽(前 20 個字,按字元數而非位元組,CJK 安全)。
fn prompt_preview(prompt: &str) -> String {
    prompt.chars().take(20).collect()
}

/// 任務結束後的佇列接續:take_next 在單一鎖內清掉 running 並彈出+預約下一個,
/// 再於鎖外啟動。啟動失敗時 do_launch 會自行清預約並接續再下一個,
/// 這裡只負責通知失敗訊息,佇列不會卡死。
pub fn start_or_queue_next(app: &AppHandle) {
    let state = app.state::<AppState>();
    let next = take_next(&state);
    emit_queue_changed(app);
    if let Some(task) = next {
        crate::notify(app, &crate::i18n::task_starting(&prompt_preview(&task.prompt)));
        if let Err(e) = do_launch(app, task) {
            crate::notify(app, &e);
        }
    }
}

fn emit_queue_changed(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("palette") {
        let _ = w.emit("queue-changed", ());
    }
}

/// 啟動「已預約」的任務(前置條件:running 已是該任務的預約 — 由
/// reserve_or_enqueue / pop_and_reserve_next 建立)。任何失敗都會在同一把鎖內
/// 清掉預約並原子接續下一個佇列任務(pop_and_reserve_next),佇列不會卡死;
/// 原始錯誤仍回傳給呼叫者(submit_prompt 顯示於面板、start_or_queue_next 通知)。
pub fn do_launch(app: &AppHandle, task: QueuedTask) -> Result<(), String> {
    let state = app.state::<AppState>();
    match try_launch(app, &state, task) {
        Ok(()) => {
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
        Err(e) => {
            // 預約失效:清掉 running 並原子彈出+預約下一個,於鎖外接續啟動
            let next = pop_and_reserve_next(&mut state.queue.lock().unwrap());
            emit_queue_changed(app);
            if let Some(next_task) = next {
                crate::notify(app, &crate::i18n::task_starting(&prompt_preview(&next_task.prompt)));
                if let Err(chain_err) = do_launch(app, next_task) {
                    crate::notify(app, &chain_err);
                }
            }
            Err(e)
        }
    }
}

/// do_launch 的實際啟動段:spawn 成功後在 queue 鎖內把 pid/background 補進
/// 既有預約;Err 一律交給 do_launch 統一清預約並接續佇列。
fn try_launch(app: &AppHandle, state: &AppState, task: QueuedTask) -> Result<(), String> {
    let s = state.settings.lock().unwrap().clone();
    let cat = state.catalog_cache.lock().unwrap().clone();
    let (model, notice) = resolve_usable_model(&s, &cat);
    if let Some(n) = notice {
        crate::notify(app, &n);
    }
    // 自動切換非 claude 模型時把新模型寫回設定(讓 chip/✓/下次啟動一致)
    if model != s.model && model != catalog::CLAUDE_MODEL {
        let mut st = state.settings.lock().unwrap();
        st.model = model.clone();
        let _ = settings::save(&settings::settings_path(), &st);
    }
    let spec = launcher::build_launch_spec(&task.prompt, &s, &model);
    let is_background = spec.background;
    // 無腦模式(非謹慎):前景互動啟動前先預先信任工作目錄,免去 Claude Code
    // 「資料夾信任」確認(背景 -p 模式本來就自動信任)。謹慎模式保留該確認當把關。
    if !s.cautious_mode && !is_background {
        crate::trust::ensure_trusted(&spec.cwd);
    }
    let dir = logging::logs_dir();
    logging::rotate(&dir, 30);
    let log = logging::new_run_log(&dir).map_err(|e| crate::i18n::log_create_failed(&e.to_string()))?;
    let app2 = app.clone();
    // 兩種模式 waiter 都會呼叫(spawn v1.1 契約):先處理通知,再接續佇列。
    let on_done: launcher::OnDone = Box::new(move |code, log_path, elapsed| {
        handle_task_exit(&app2, code, &log_path, elapsed, is_background, &model);
        start_or_queue_next(&app2);
    });
    {
        // 持鎖橫跨 spawn → 補 pid:waiter(handle_task_exit/start_or_queue_next)
        // 第一步就鎖 queue,因此「極速結束」也不會在 pid 寫入前清掉預約。
        let mut q = state.queue.lock().unwrap();
        let pid = launcher::spawn(&spec, log, Some(on_done)).map_err(|e| crate::i18n::launch_failed(&e.to_string()))?;
        if let Some(rt) = q.running.as_mut().filter(|rt| rt.id == task.id) {
            rt.pid = Some(pid);
            rt.background = is_background;
        }
    }
    Ok(())
}

/// 退出中的任務是否為使用者主動停止(task_stop 標記)。
/// 必須在 waiter 呼叫 start_or_queue_next 清掉 running「之前」讀取。
fn task_was_stopping(state: &AppState) -> bool {
    state.queue.lock().unwrap().running.as_ref().is_some_and(|rt| rt.stopping)
}

/// 任務結束通知決策(原本散在 launcher.rs 的 fast-fail 判斷移到這裡):
/// - 使用者主動停止(stopping)→ 「已停止任務」,不跑失敗分類
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
    // M3:taskkill 的退出碼會被分類成「任務失敗」— 使用者主動停止不是失敗。
    // 佇列接續由呼叫端(start_or_queue_next)照常進行。
    if task_was_stopping(&app.state::<AppState>()) {
        crate::notify(app, &crate::i18n::task_stopped());
        return;
    }
    if code == 0 {
        // 被動學習:exit 0 代表模型免費可用 —— 但只在「背景模式」採信。
        // 前景是 `ollama launch` 包裝 claude 的退出碼,使用者關掉主控台或包裝器
        // 遮蔽子程序錯誤都可能回 0(誤判);背景 -p 模式下模型 403/失敗時 claude
        // 會以非零結束,所以 exit 0 才是可靠訊號。claude(Anthropic 帳號)不適用。
        if is_background && model != catalog::CLAUDE_MODEL {
            let state = app.state::<AppState>();
            let mut st = state.settings.lock().unwrap();
            if !st.known_free_models.iter().any(|m| m == model) {
                st.known_free_models.push(model.to_string());
                let _ = settings::save(&settings::settings_path(), &st);
            }
        }
        if is_background {
            crate::notify(app, &crate::i18n::task_done());
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
        crate::notify(app, &crate::i18n::fast_fail(code));
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
            crate::notify(app, &crate::i18n::subscription_required());
        }
        launcher::FailureKind::Quota => {
            crate::notify(app, &crate::i18n::quota_exhausted());
        }
        launcher::FailureKind::Auth => {
            // Re-lock sign-in state so next submit_prompt triggers the wizard
            let state = app.state::<AppState>();
            {
                let mut st = state.settings.lock().unwrap();
                st.signin_state = SigninState::No;
                let _ = settings::save(&settings::settings_path(), &st);
            }
            crate::notify(app, &crate::i18n::need_relogin());
        }
        launcher::FailureKind::Other => {
            let log_str = log_path.display().to_string();
            let msg = if code == -1 {
                crate::i18n::task_crashed(&log_str)
            } else {
                crate::i18n::task_failed(code, &log_str)
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
    let model = {
        let s = state.settings.lock().unwrap();
        crate::i18n::set_locale(&s.locale);
        s.model.clone()
    };
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
                    bootstrap::StepResult { ok: false, detail: crate::i18n::ollama_not_ready() }
                } else if step2 == "signin" {
                    bootstrap::signin(&runner)
                } else {
                    bootstrap::register_model(&runner, &model)
                }
            }
            other => bootstrap::StepResult { ok: false, detail: crate::i18n::unknown_step(other) },
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
    let q = state.queue.lock().unwrap();
    QueueDto { running: q.running.clone(), queued: q.queued.iter().cloned().collect() }
}

/// 純佇列變更:移除指定 id,回傳是否有移除。
fn cancel_in_queue(state: &AppState, id: u64) -> bool {
    let mut q = state.queue.lock().unwrap();
    let before = q.queued.len();
    q.queued.retain(|t| t.id != id);
    q.queued.len() != before
}

#[tauri::command]
pub fn queue_cancel(state: State<AppState>, app: AppHandle, id: u64) -> Result<(), String> {
    if !cancel_in_queue(&state, id) {
        return Err(crate::i18n::task_not_in_queue());
    }
    emit_queue_changed(&app);
    Ok(())
}

/// 純決策+標記(M4):在「同一次持鎖」中完成 — 確認 running 仍是預期任務
/// (id 不符 → 任務已換手)、套用停止規則、標記 stopping(M3)並回傳
/// (id, pid) 供鎖外 taskkill。讀 pid 與標記之間沒有空窗,不會殺錯任務。
fn mark_stopping(q: &mut QueueState, expected_id: Option<u64>) -> Result<(u64, u32), String> {
    let Some(rt) = q.running.as_mut() else {
        return Err(crate::i18n::no_running_task());
    };
    if expected_id.is_some_and(|id| id != rt.id) {
        return Err(crate::i18n::task_already_ended());
    }
    if !rt.background {
        return Err(crate::i18n::foreground_close_terminal());
    }
    let pid = rt.pid.ok_or_else(crate::i18n::no_task_pid)?;
    rt.stopping = true;
    Ok((rt.id, pid))
}

/// 停止背景任務:taskkill 整個 process tree;不在這裡清 running —
/// waiter 執行緒的 on_done 會自然觸發並接續佇列。
/// id 為前端「想停的那個任務」(可省略 = 停目前執行中的);若任務已換手
/// 回 Err,不會誤殺接續的任務。
#[tauri::command]
pub fn task_stop(state: State<AppState>, _app: AppHandle, id: Option<u64>) -> Result<(), String> {
    let (task_id, pid) = mark_stopping(&mut state.queue.lock().unwrap(), id)?;
    use crate::command::Runner as _;
    if let Err(e) = SystemRunner.spawn_detached("taskkill", &["/PID", &pid.to_string(), "/T", "/F"]) {
        // taskkill 沒送出 → 撤銷標記,任務之後的自然結束不該被報成「已停止」
        if let Some(rt) = state.queue.lock().unwrap().running.as_mut().filter(|rt| rt.id == task_id) {
            rt.stopping = false;
        }
        return Err(e.to_string());
    }
    Ok(())
}

// ---------- v1.1: model selection + tier learning ----------

#[derive(serde::Serialize)]
pub struct ModelEntry {
    pub name: String,
    pub tier: String,
}

/// tier 來自五個來源,依此優先序判定:
/// "anthropic":claude 哨符(用 Anthropic 帳號,永不探測);
/// "broken":掃描學到「無法使用」(優先於 free,避免 learned-broken 被遮蓋);
/// "subscription":實證訂閱名單或執行時/掃描學到要訂閱;
/// "free":實證免費名單或被動學習/掃描學到免費;其餘 "unknown"。
fn compute_tier(
    name: &str,
    known_subscription: &[String],
    known_free: &[String],
    known_broken: &[String],
) -> &'static str {
    if name == catalog::CLAUDE_MODEL {
        "anthropic"
    } else if catalog::VERIFIED_INCOMPATIBLE.contains(&name) {
        // 免費可連但跑不動 Claude Code — 最高優先,避免被誤標成 free
        "incompatible"
    } else if known_broken.iter().any(|m| m == name) {
        "broken"
    } else if catalog::VERIFIED_SUBSCRIPTION.contains(&name) || known_subscription.iter().any(|m| m == name) {
        "subscription"
    } else if catalog::VERIFIED_FREE.contains(&name) || known_free.iter().any(|m| m == name) {
        "free"
    } else {
        "unknown"
    }
}

/// 解析「實際要用的模型」:先處理目錄 fallback(choose_model),再做 tier 檢查 —
/// 若選中的模型不可用(incompatible/subscription/broken),自動改用目錄中第一個
/// 免費模型(優先 FALLBACKS 順序)。回傳 (model, 若有自動切換的通知)。
/// claude(Anthropic 帳號)永遠原樣。
fn resolve_usable_model(s: &Settings, catalog: &[String]) -> (String, Option<String>) {
    if s.model == catalog::CLAUDE_MODEL {
        return (catalog::CLAUDE_MODEL.to_string(), None);
    }
    let (model, notice) = catalog::choose_model(&s.model, catalog);
    let tier = compute_tier(
        &model,
        &s.known_subscription_models,
        &s.known_free_models,
        &s.known_broken_models,
    );
    if matches!(tier, "incompatible" | "subscription" | "broken") {
        let pick = catalog::FALLBACKS
            .iter()
            .find(|f| catalog.iter().any(|c| c == *f))
            .map(|f| f.to_string())
            .or_else(|| {
                catalog
                    .iter()
                    .find(|m| {
                        compute_tier(
                            m,
                            &s.known_subscription_models,
                            &s.known_free_models,
                            &s.known_broken_models,
                        ) == "free"
                    })
                    .cloned()
            });
        if let Some(p) = pick {
            if p != model {
                let n = crate::i18n::model_auto_switched(&model, &p, tier);
                return (p, Some(n));
            }
        }
    }
    (model, notice)
}

#[tauri::command]
pub fn list_models_ui(state: State<AppState>) -> Vec<ModelEntry> {
    let (current, known_sub, known_free, known_broken) = {
        let s = state.settings.lock().unwrap();
        (
            s.model.clone(),
            s.known_subscription_models.clone(),
            s.known_free_models.clone(),
            s.known_broken_models.clone(),
        )
    };
    let cat = state.catalog_cache.lock().unwrap().clone();
    // 離線/尚未抓到目錄時至少回傳目前設定的模型
    let names = if cat.is_empty() { vec![current] } else { cat };
    // claude(Anthropic 帳號)永遠置頂,且不論目錄是否抓到都列出
    let mut entries = vec![ModelEntry {
        name: catalog::CLAUDE_MODEL.to_string(),
        tier: "anthropic".to_string(),
    }];
    entries.extend(
        names
            .into_iter()
            .filter(|n| n != catalog::CLAUDE_MODEL)
            .map(|name| {
                let tier = compute_tier(&name, &known_sub, &known_free, &known_broken).to_string();
                ModelEntry { name, tier }
            }),
    );
    entries
}

#[tauri::command]
pub async fn set_model(_app: AppHandle, state: State<'_, AppState>, name: String) -> Result<(), String> {
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err(crate::i18n::empty_model_name());
    }
    let mut s = state.settings.lock().unwrap();
    s.model = name;
    settings::save(&settings::settings_path(), &s).map_err(|e| e.to_string())
}

// ---------- v1.1: on-demand availability scan ----------

pub const OLLAMA_CHAT_URL: &str = "http://127.0.0.1:11434/api/chat";

#[derive(serde::Serialize)]
pub struct ScanSummary {
    pub free: usize,
    pub subscription: usize,
    pub broken: usize,
    pub scanned: usize,
    pub skipped: usize,
}

/// 把 /api/chat 探測結果映射成 tier:200=免費、403=需訂閱、其餘 HTTP 狀態=無法使用。
/// 連線錯誤(transport Err)回 None —— 代表「測不到」(多半是 Ollama 沒開),
/// 不可寫入 known_broken(否則暫時斷線會永久汙染快取),維持 unknown 待重掃。
fn classify_probe(result: Result<u16, String>) -> Option<&'static str> {
    match result {
        Ok(200) => Some("free"),
        Ok(403) => Some("subscription"),
        Ok(_) => Some("broken"),
        Err(_) => None,
    }
}

/// /api/chat 探測用的最小請求 body(num_predict:1 只要 1 個 token,省額度)。
/// 用 serde_json 組,安全跳脫模型名稱。
fn probe_body(model: &str) -> String {
    serde_json::json!({
        "model": model,
        "messages": [{ "role": "user", "content": "hi" }],
        "stream": false,
        "options": { "num_predict": 1 }
    })
    .to_string()
}

/// dedupe push:不存在才加入。
fn push_unique(list: &mut Vec<String>, model: &str) {
    if !list.iter().any(|m| m == model) {
        list.push(model.to_string());
    }
}

/// 把模型從某個名單移除(重分類時用)。
fn remove_from(list: &mut Vec<String>, model: &str) {
    list.retain(|m| m != model);
}

/// 主動掃描:對目錄中 tier 仍為 "unknown" 的模型逐一探測 /api/chat,
/// 依 200/403/其餘 分類成 free/subscription/broken 寫回設定。
/// 探測在 spawn_blocking 內進行(不可橫跨 await 持有 State),先把需要的資料
/// clone 出來,探完再鎖 settings 合併+存檔一次。進度經由 cloned AppHandle
/// emit "scan-progress",結束 emit "scan-done"。
#[tauri::command]
pub async fn scan_models(app: AppHandle, state: State<'_, AppState>) -> Result<ScanSummary, String> {
    // (1) 快照目錄 + 目前的 known_* 名單(鎖外探測前先 clone)
    let catalog_models = state.catalog_cache.lock().unwrap().clone();
    let (known_sub, known_free, known_broken) = {
        let s = state.settings.lock().unwrap();
        (
            s.known_subscription_models.clone(),
            s.known_free_models.clone(),
            s.known_broken_models.clone(),
        )
    };
    // (2) 目標 = 目錄中 tier 仍為 "unknown" 的模型(跳過 claude / 已分類者)
    let total = catalog_models.len();
    let targets: Vec<String> = catalog_models
        .into_iter()
        .filter(|n| n != catalog::CLAUDE_MODEL)
        .filter(|n| compute_tier(n, &known_sub, &known_free, &known_broken) == "unknown")
        .collect();
    let skipped = total - targets.len();
    let target_total = targets.len();

    // (3) 在 spawn_blocking 內逐一 pull + probe + 分類 + emit 進度
    let app2 = app.clone();
    let results: Vec<(String, &'static str)> = tauri::async_runtime::spawn_blocking(move || {
        use crate::command::Runner as _;
        use crate::http::Http as _;
        let runner = SystemRunner;
        let http = UreqHttp;
        let mut out: Vec<(String, &'static str)> = Vec::with_capacity(target_total);
        for (i, model) in targets.into_iter().enumerate() {
            // pull 先註冊雲端 stub(忽略結果);60s 上限
            let _ = runner.run("ollama", &["pull", &model], std::time::Duration::from_secs(60));
            let status = http.post_status(OLLAMA_CHAT_URL, &probe_body(&model), std::time::Duration::from_secs(60));
            let is_transport_err = status.is_err();
            let tier = classify_probe(status);
            // 第一個就連不上 → Ollama 多半沒開,直接中止(不寫入任何 broken)
            if is_transport_err && out.is_empty() {
                return Err(crate::i18n::ollama_not_responding_for_scan());
            }
            // 連線錯誤(None)= 測不到,跳過不寫入;只有確定的 HTTP 結果才記錄
            if let Some(tier) = tier {
                out.push((model.clone(), tier));
            }
            if let Some(w) = app2.get_webview_window("palette") {
                let _ = w.emit(
                    "scan-progress",
                    serde_json::json!({
                        "done": i + 1,
                        "total": target_total,
                        "model": model,
                        "result": tier.unwrap_or("error")
                    }),
                );
            }
        }
        Ok(out)
    })
    .await
    .map_err(|e| e.to_string())??;

    // (4) 合併回設定(dedupe;重分類時從另外兩個名單移除),存檔一次
    let mut summary = ScanSummary { free: 0, subscription: 0, broken: 0, scanned: results.len(), skipped };
    {
        let mut s = state.settings.lock().unwrap();
        for (model, tier) in &results {
            match *tier {
                "free" => {
                    remove_from(&mut s.known_subscription_models, model);
                    remove_from(&mut s.known_broken_models, model);
                    push_unique(&mut s.known_free_models, model);
                    summary.free += 1;
                }
                "subscription" => {
                    remove_from(&mut s.known_free_models, model);
                    remove_from(&mut s.known_broken_models, model);
                    push_unique(&mut s.known_subscription_models, model);
                    summary.subscription += 1;
                }
                _ => {
                    remove_from(&mut s.known_free_models, model);
                    remove_from(&mut s.known_subscription_models, model);
                    push_unique(&mut s.known_broken_models, model);
                    summary.broken += 1;
                }
            }
        }
        settings::save(&settings::settings_path(), &s).map_err(|e| e.to_string())?;
    }

    // (5) 通知前端掃描完成(前端據此重抓 list_models_ui)
    if let Some(w) = app.get_webview_window("palette") {
        let _ = w.emit(
            "scan-done",
            serde_json::json!({
                "free": summary.free,
                "subscription": summary.subscription,
                "broken": summary.broken,
                "scanned": summary.scanned,
                "skipped": summary.skipped
            }),
        );
    }
    Ok(summary)
}

/// webview 不得開任意網址 — 僅允許白名單(完全比對)。
fn url_allowed(url: &str) -> bool {
    const ALLOWED: [&str; 2] = ["https://ollama.com/settings", "https://ollama.com/upgrade"];
    ALLOWED.contains(&url)
}

#[tauri::command]
pub fn open_url(url: String) -> Result<(), String> {
    if !url_allowed(&url) {
        return Err(crate::i18n::url_not_allowed());
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

/// refresh_plan 的 in-flight 防護(M2):get_status 每 5 秒輪詢都可能觸發,
/// 抓取最長 5 秒 — 沒有防護會堆疊出多個並行的 /api/me 請求。
static PLAN_FETCH_INFLIGHT: AtomicBool = AtomicBool::new(false);

/// true = 取得抓取權(原 false);false = 已有人在抓,跳過。
fn try_begin_plan_fetch() -> bool {
    !PLAN_FETCH_INFLIGHT.swap(true, Ordering::SeqCst)
}

fn end_plan_fetch() {
    PLAN_FETCH_INFLIGHT.store(false, Ordering::SeqCst);
}

/// 非阻塞更新帳號方案快取;啟動時呼叫一次,get_status 在快取為空時也會補觸發。
/// 同一時間只允許一個 in-flight 抓取(成功或失敗皆會釋放)。
pub fn refresh_plan(app: AppHandle) {
    if !try_begin_plan_fetch() {
        return;
    }
    tauri::async_runtime::spawn_blocking(move || {
        let plan = fetch_plan(&UreqHttp);
        if plan.is_some() {
            let state = app.state::<AppState>();
            *state.plan.lock().unwrap() = plan;
        }
        end_plan_fetch();
    });
}

// ---------- v1.1: voice input (Win+H) ----------

/// 確保輸入面板可見且取得焦點後,送出 Win+H 啟動 Windows 語音輸入。
/// 整段流程(聚焦 → 50ms 等焦點切換 → 等實體按鍵放開 → SendInput)都會阻塞,
/// 因此跑在 spawn_blocking,不佔住 async runtime。
/// 聚焦失敗時「不注入」— 否則 Win+H 會落在別的視窗。
#[tauri::command]
pub async fn start_voice_input(app: AppHandle) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let Some(w) = app.get_webview_window("palette") else {
            return Err(crate::i18n::palette_window_not_found());
        };
        let _ = w.show();
        w.set_focus().map_err(|_| crate::i18n::palette_focus_failed())?;
        // 50ms 延遲讓焦點切換先完成,語音輸入才會落在面板的輸入框
        std::thread::sleep(std::time::Duration::from_millis(50));
        if let Err(e) = crate::voice::trigger_voice_typing() {
            // 熱鍵路徑(lib.rs)會丟棄回傳值 — 失敗必須額外以通知浮現
            crate::notify(&app, &e);
            return Err(e);
        }
        Ok(())
    })
    .await
    .map_err(|e| e.to_string())?
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
    // pid 回填、on_done 通知與佇列接續(start_or_queue_next 經由真實
    // waiter 執行緒觸發)只能由 E2E 驗證。單元測試鎖定其中的純邏輯:
    // 佇列狀態機(reserve_or_enqueue/pop_and_reserve_next/take_next/
    // cancel_in_queue)、停止決策+標記(mark_stopping/task_was_stopping)、
    // 設定 overlay、plan in-flight 防護、tier 計算、URL 白名單、/api/me 解析。

    fn state() -> AppState {
        AppState::new(Settings::default())
    }

    fn running_task(id: u64, background: bool, pid: Option<u32>) -> RunningTask {
        RunningTask { id, prompt: "busy".into(), background, pid, stopping: false }
    }

    fn enqueue_prompt(st: &AppState, prompt: &str) -> QueuedTask {
        let task = new_task(st, prompt.into());
        assert!(reserve_or_enqueue(st, task.clone()).is_none(), "running 中應入列");
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
    fn reserve_or_enqueue_launches_directly_when_idle() {
        let st = state();
        let task = new_task(&st, "hi".into());
        // 沒有 running → 任務交還呼叫者啟動,佇列保持空
        let returned = reserve_or_enqueue(&st, task.clone());
        assert_eq!(returned, Some(task));
        assert!(st.queue.lock().unwrap().queued.is_empty());
    }

    /// C1 釘住原子性 (a):idle 時的決策必須「當下就預約」running 槽位 —
    /// 緊接著的第二個決策(double-submit)只能入列,不可能並行執行。
    #[test]
    fn reserve_or_enqueue_reserves_running_slot_atomically() {
        let st = state();
        let a = new_task(&st, "first".into());
        let b = new_task(&st, "second".into());
        let launched = reserve_or_enqueue(&st, a.clone());
        assert_eq!(launched, Some(a.clone()));
        {
            let q = st.queue.lock().unwrap();
            let rt = q.running.as_ref().expect("決策當下就必須寫入預約");
            assert_eq!(rt.id, a.id);
            assert_eq!(rt.prompt, a.prompt);
            assert_eq!(rt.pid, None, "pid 由 do_launch 在 spawn 後補上");
            assert!(!rt.stopping);
        }
        let b_id = b.id;
        assert_eq!(reserve_or_enqueue(&st, b), None, "預約存在 → 第二個決策必須入列");
        let q = st.queue.lock().unwrap();
        assert_eq!(q.queued.iter().map(|t| t.id).collect::<Vec<_>>(), vec![b_id]);
        assert_eq!(q.running.as_ref().map(|rt| rt.id), Some(a.id), "預約不得被第二個決策覆蓋");
    }

    #[test]
    fn reserve_or_enqueue_queues_fifo_while_running() {
        let st = state();
        st.queue.lock().unwrap().running = Some(running_task(99, true, Some(1234)));
        let a = enqueue_prompt(&st, "first");
        let b = enqueue_prompt(&st, "second");
        let c = enqueue_prompt(&st, "third");
        let q: Vec<u64> = st.queue.lock().unwrap().queued.iter().map(|t| t.id).collect();
        assert_eq!(q, vec![a.id, b.id, c.id], "佇列必須維持 FIFO 順序");
    }

    #[test]
    fn cancel_removes_only_the_requested_id() {
        let st = state();
        st.queue.lock().unwrap().running = Some(running_task(99, true, None));
        let a = enqueue_prompt(&st, "first");
        let b = enqueue_prompt(&st, "second");
        let c = enqueue_prompt(&st, "third");
        assert!(cancel_in_queue(&st, b.id));
        let q: Vec<u64> = st.queue.lock().unwrap().queued.iter().map(|t| t.id).collect();
        assert_eq!(q, vec![a.id, c.id]);
        assert!(!cancel_in_queue(&st, b.id), "重複取消同 id 應回報找不到");
        assert!(!cancel_in_queue(&st, 424242), "不存在的 id 應回報找不到");
    }

    #[test]
    fn take_next_pops_fifo_and_clears_running_when_queue_empty() {
        let st = state();
        st.queue.lock().unwrap().running = Some(running_task(99, true, Some(1)));
        let a = enqueue_prompt(&st, "first");
        let b = enqueue_prompt(&st, "second");
        let next = take_next(&st).unwrap();
        assert_eq!(next.id, a.id);
        assert_eq!(take_next(&st).unwrap().id, b.id);
        assert!(take_next(&st).is_none(), "佇列清空後應回 None");
        assert!(st.queue.lock().unwrap().running.is_none(), "沒有下一個任務時必須清掉 running");
    }

    /// C1 釘住原子性 (b):take_next 在同一把鎖內「彈出+預約」— 彈出
    /// 之後緊接著的決策必須看到預約而入列,不會與彈出的任務並行。
    #[test]
    fn take_next_pops_and_reserves_atomically() {
        let st = state();
        st.queue.lock().unwrap().running = Some(running_task(99, true, Some(1)));
        let a = enqueue_prompt(&st, "first");
        let b = enqueue_prompt(&st, "second");
        let next = take_next(&st).unwrap();
        assert_eq!(next.id, a.id);
        {
            let q = st.queue.lock().unwrap();
            let rt = q.running.as_ref().expect("take_next 必須同時預約彈出的任務");
            assert_eq!(rt.id, a.id);
            assert_eq!(rt.pid, None, "pid 由 do_launch 在 spawn 後補上");
            assert!(!rt.stopping);
        }
        let c = new_task(&st, "third".into());
        let c_id = c.id;
        assert_eq!(reserve_or_enqueue(&st, c), None, "彈出後緊接著的決策必須入列");
        let order: Vec<u64> = st.queue.lock().unwrap().queued.iter().map(|t| t.id).collect();
        assert_eq!(order, vec![b.id, c_id]);
    }

    #[test]
    fn mark_stopping_rules_and_sets_flag_in_same_acquisition() {
        // 這些斷言比對的是經 i18n(預設繁中)產生的字串;固定語系避免與
        // i18n 測試在同一 process 平行設定 en 時互相干擾。
        crate::i18n::set_locale("zh-TW");
        let mut q = QueueState::default();
        assert!(mark_stopping(&mut q, None).is_err(), "沒有執行中任務不能停止");
        q.running = Some(running_task(1, false, Some(10)));
        assert_eq!(mark_stopping(&mut q, None).unwrap_err(), "前景任務請直接關閉其終端機視窗");
        q.running = Some(running_task(3, true, None));
        assert!(mark_stopping(&mut q, None).is_err(), "無 pid 無法停止");
        assert!(!q.running.as_ref().unwrap().stopping, "失敗路徑不得標記 stopping");
        q.running = Some(running_task(2, true, Some(4321)));
        assert_eq!(mark_stopping(&mut q, None).unwrap(), (2, 4321));
        assert!(q.running.as_ref().unwrap().stopping, "成功時必須同次持鎖標記 stopping");
    }

    /// M4:任務已換手(running.id != 預期 id)→ 不可殺到接續的新任務。
    #[test]
    fn mark_stopping_rejects_stale_task_id() {
        crate::i18n::set_locale("zh-TW");
        let mut q = QueueState { running: Some(running_task(7, true, Some(111))), ..Default::default() };
        assert_eq!(mark_stopping(&mut q, Some(6)).unwrap_err(), "任務已結束");
        assert!(!q.running.as_ref().unwrap().stopping, "換手時不得標記新任務");
        assert_eq!(mark_stopping(&mut q, Some(7)).unwrap(), (7, 111), "id 相符照常停止");
    }

    /// M3:handle_task_exit 以此判斷「使用者主動停止」→ 不報任務失敗。
    #[test]
    fn task_was_stopping_reads_flag_from_running() {
        let st = state();
        assert!(!task_was_stopping(&st), "沒有 running → 非主動停止");
        st.queue.lock().unwrap().running = Some(running_task(1, true, Some(42)));
        assert!(!task_was_stopping(&st));
        st.queue.lock().unwrap().running.as_mut().unwrap().stopping = true;
        assert!(task_was_stopping(&st));
    }

    /// M1:save_settings 只接受 6 個 UI 欄位;server-side 欄位以記憶體現值為準。
    #[test]
    fn overlay_ui_fields_keeps_server_side_state() {
        let current = Settings {
            history: vec!["真實歷史".into()],
            signin_state: SigninState::Yes,
            known_subscription_models: vec!["minimax-m2.7:cloud".into()],
            known_free_models: vec!["gemma3:4b-cloud".into()],
            known_broken_models: vec!["dead:cloud".into()],
            ..Default::default()
        };
        let incoming = Settings {
            hotkey: "Ctrl+Alt+Space".into(),
            model: "qwen3-coder-next:cloud".into(),
            cautious_mode: true,
            background_mode: true,
            working_dir: r"C:\work".into(),
            autostart: false,
            locale: "en".into(),
            system_prompt: "自訂個性".into(),
            // 前端的舊快照 — 必須被忽略
            history: vec!["stale".into()],
            signin_state: SigninState::No,
            known_subscription_models: vec!["stale:cloud".into()],
            known_free_models: vec!["stale-free:cloud".into()],
            known_broken_models: vec!["stale-broken:cloud".into()],
        };
        let merged = overlay_ui_fields(&current, &incoming);
        assert_eq!(merged.hotkey, "Ctrl+Alt+Space");
        assert_eq!(merged.model, "qwen3-coder-next:cloud");
        assert!(merged.cautious_mode);
        assert!(merged.background_mode);
        assert_eq!(merged.working_dir, r"C:\work");
        assert!(!merged.autostart);
        assert_eq!(merged.locale, "en", "locale 屬 UI 欄位,以 incoming 為準");
        assert_eq!(merged.system_prompt, "自訂個性", "system_prompt 屬 UI 欄位,以 incoming 為準");
        assert_eq!(merged.history, vec!["真實歷史".to_string()], "history 以記憶體為準");
        assert_eq!(merged.signin_state, SigninState::Yes, "signin_state 以記憶體為準");
        assert_eq!(
            merged.known_subscription_models,
            vec!["minimax-m2.7:cloud".to_string()],
            "tier learning 以記憶體為準"
        );
        assert_eq!(
            merged.known_free_models,
            vec!["gemma3:4b-cloud".to_string()],
            "被動學習的免費名單以記憶體為準"
        );
        assert_eq!(
            merged.known_broken_models,
            vec!["dead:cloud".to_string()],
            "掃描到的無法使用名單以記憶體為準"
        );
    }

    #[test]
    fn sanitize_image_ext_allows_known_and_defaults_others() {
        assert_eq!(sanitize_image_ext("PNG"), "png");
        assert_eq!(sanitize_image_ext("jpeg"), "jpeg");
        assert_eq!(sanitize_image_ext("webp"), "webp");
        // 不在白名單 / 含路徑字元 → 一律 png(防止任意副檔名)
        assert_eq!(sanitize_image_ext("exe"), "png");
        assert_eq!(sanitize_image_ext("../evil"), "png");
        assert_eq!(sanitize_image_ext(""), "png");
    }

    /// M2:同一時間只允許一個 in-flight plan 抓取;結束後可再抓。
    #[test]
    fn plan_fetch_inflight_guard_blocks_second_and_clears() {
        assert!(try_begin_plan_fetch(), "閒置時可開始抓取");
        assert!(!try_begin_plan_fetch(), "in-flight 期間必須跳過");
        end_plan_fetch();
        assert!(try_begin_plan_fetch(), "結束(成功或失敗)後可再次抓取");
        end_plan_fetch();
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
    fn compute_tier_free_subscription_broken_unknown() {
        let none: Vec<String> = vec![];
        // 實證免費名單
        assert_eq!(compute_tier("minimax-m2.5:cloud", &none, &none, &none), "free");
        assert_eq!(compute_tier("qwen3-coder-next:cloud", &none, &none, &none), "free");
        assert_eq!(compute_tier("glm-4.7:cloud", &none, &none, &none), "free");
        assert_eq!(compute_tier("gemma4:31b-cloud", &none, &none, &none), "free");
        assert_eq!(compute_tier("gpt-oss:120b-cloud", &none, &none, &none), "free");
        // 實證訂閱名單(VERIFIED_SUBSCRIPTION)
        assert_eq!(compute_tier("deepseek-v3.2:cloud", &none, &none, &none), "subscription");
        assert_eq!(compute_tier("minimax-m2.7:cloud", &none, &none, &none), "subscription");
        // 學到的訂閱模型
        let known_sub = vec!["custom-sub:cloud".to_string()];
        assert_eq!(compute_tier("custom-sub:cloud", &known_sub, &none, &none), "subscription");
        // 被動學習/掃描學到的免費模型
        let known_free = vec!["learned-free:cloud".to_string()];
        assert_eq!(compute_tier("learned-free:cloud", &none, &known_free, &none), "free");
        // 掃描學到的無法使用模型
        let known_broken = vec!["dead:cloud".to_string()];
        assert_eq!(compute_tier("dead:cloud", &none, &none, &known_broken), "broken");
        // 完全未知
        assert_eq!(compute_tier("nobody-knows:cloud", &none, &none, &none), "unknown");
        // claude 哨符
        assert_eq!(compute_tier("claude", &none, &none, &none), "anthropic");
        // 不支援 Claude Code(免費可連但跑不動)
        assert_eq!(compute_tier("rnj-1:8b-cloud", &none, &none, &none), "incompatible");
        // 優先序:broken 蓋過 free(learned-broken 不被實證免費遮蓋)
        let broken_qwen = vec!["qwen3-coder-next:cloud".to_string()];
        assert_eq!(compute_tier("qwen3-coder-next:cloud", &none, &none, &broken_qwen), "broken");
        // 優先序:subscription 蓋過 free
        let sub_qwen = vec!["qwen3-coder-next:cloud".to_string()];
        assert_eq!(compute_tier("qwen3-coder-next:cloud", &sub_qwen, &none, &none), "subscription");
    }

    #[test]
    fn resolve_usable_model_switches_away_from_unusable() {
        let cat: Vec<String> = vec![
            "rnj-1:8b-cloud".into(),
            "deepseek-v3.2:cloud".into(),
            "minimax-m2.5:cloud".into(),
            "qwen3-coder-next:cloud".into(),
        ];
        // 不支援 → 換成第一個免費(FALLBACKS[0] = minimax-m2.5,在目錄內)
        let s = Settings { model: "rnj-1:8b-cloud".into(), ..Default::default() };
        let (m, notice) = resolve_usable_model(&s, &cat);
        assert_eq!(m, "minimax-m2.5:cloud");
        assert!(notice.is_some());
        // 需訂閱 → 同樣自動換走
        let s = Settings { model: "deepseek-v3.2:cloud".into(), ..Default::default() };
        let (m, _) = resolve_usable_model(&s, &cat);
        assert_eq!(m, "minimax-m2.5:cloud");
        // 本來就免費 → 不動、無通知
        let s = Settings { model: "qwen3-coder-next:cloud".into(), ..Default::default() };
        let (m, notice) = resolve_usable_model(&s, &cat);
        assert_eq!(m, "qwen3-coder-next:cloud");
        assert!(notice.is_none());
        // claude → 永遠原樣
        let s = Settings { model: "claude".into(), ..Default::default() };
        let (m, notice) = resolve_usable_model(&s, &cat);
        assert_eq!(m, "claude");
        assert!(notice.is_none());
    }

    #[test]
    fn classify_probe_maps_status_to_tier() {
        assert_eq!(classify_probe(Ok(200)), Some("free"));
        assert_eq!(classify_probe(Ok(403)), Some("subscription"));
        assert_eq!(classify_probe(Ok(500)), Some("broken"));
        assert_eq!(classify_probe(Ok(404)), Some("broken"));
        // 連線錯誤不得分類成 broken(會永久汙染快取);回 None 維持 unknown 待重掃
        assert_eq!(classify_probe(Err("connection refused".into())), None);
    }

    #[test]
    fn probe_body_escapes_model_name_and_limits_tokens() {
        let body = probe_body(r#"weird"name:cloud"#);
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["model"], r#"weird"name:cloud"#);
        assert_eq!(v["stream"], false);
        assert_eq!(v["options"]["num_predict"], 1);
        assert_eq!(v["messages"][0]["role"], "user");
    }

    #[test]
    fn push_unique_and_remove_from_dedupe_and_reclassify() {
        let mut list = vec!["a:cloud".to_string()];
        push_unique(&mut list, "a:cloud"); // already present → no dup
        push_unique(&mut list, "b:cloud");
        assert_eq!(list, vec!["a:cloud".to_string(), "b:cloud".to_string()]);
        remove_from(&mut list, "a:cloud");
        assert_eq!(list, vec!["b:cloud".to_string()]);
        remove_from(&mut list, "missing:cloud"); // no-op
        assert_eq!(list, vec!["b:cloud".to_string()]);
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
