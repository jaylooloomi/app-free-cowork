use crate::command::SystemRunner;
use crate::http::UreqHttp;
use crate::settings::{Settings, SigninState};
use crate::{bootstrap, catalog, doctor, launcher, logging, settings};
use std::sync::Mutex;
use tauri::{AppHandle, Emitter as _, Manager, State};

pub struct AppState {
    pub settings: Mutex<Settings>,
    pub pending_prompt: Mutex<Option<String>>,
    pub catalog_cache: Mutex<Vec<String>>,
}

/// Production doctor deps: real runner/http, default claude paths,
/// 200ms × 50 attempts (= wait up to 10s for `ollama serve`).
fn prod_deps<'a>(runner: &'a SystemRunner, http: &'a UreqHttp) -> doctor::Deps<'a> {
    doctor::Deps {
        runner,
        http,
        claude_paths: doctor::default_claude_paths(),
        serve_poll_ms: 200,
        serve_attempts: 50,
    }
}

#[derive(serde::Serialize)]
pub struct StatusDto {
    pub state: String,
    pub model: String,
    pub detail: String,
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
pub async fn get_status(state: State<'_, AppState>) -> Result<StatusDto, String> {
    let s = state.settings.lock().unwrap().clone();
    let cat = state.catalog_cache.lock().unwrap().clone();
    let (model, _) = catalog::choose_model(&s.model, &cat);
    let status = tauri::async_runtime::spawn_blocking(move || {
        let runner = SystemRunner;
        let http = UreqHttp;
        doctor::quick_check(&prod_deps(&runner, &http))
    })
    .await
    .map_err(|e| e.to_string())?;
    Ok(match status {
        doctor::Status::Ready => StatusDto { state: "ready".into(), model, detail: String::new() },
        doctor::Status::NeedsSetup { .. } => StatusDto {
            state: "needs_setup".into(),
            model,
            detail: "首次使用將自動安裝必要元件".into(),
        },
        doctor::Status::Degraded { reason } => StatusDto { state: "degraded".into(), model, detail: reason },
    })
}

#[tauri::command]
pub fn get_history(state: State<AppState>) -> Vec<String> {
    state.settings.lock().unwrap().history.clone()
}

/// 回傳 "launched" | "wizard";Err(中文訊息) 顯示在面板。
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
            do_launch(&app, &state, &prompt)?;
            hide_window(&app, "palette");
            Ok("launched".into())
        }
    }
}

pub fn do_launch(app: &AppHandle, state: &State<AppState>, prompt: &str) -> Result<(), String> {
    let s = state.settings.lock().unwrap().clone();
    let cat = state.catalog_cache.lock().unwrap().clone();
    let (model, notice) = catalog::choose_model(&s.model, &cat);
    if let Some(n) = notice {
        crate::notify(app, &n);
    }
    let spec = launcher::build_launch_spec(prompt, &s, &model);
    let dir = logging::logs_dir();
    logging::rotate(&dir, 30);
    let log = logging::new_run_log(&dir).map_err(|e| format!("無法建立記錄檔:{e}"))?;
    let app2 = app.clone();
    let on_done: Option<Box<dyn FnOnce(i32, std::path::PathBuf) + Send>> = if spec.background {
        Some(Box::new(move |code, log_path| {
            let msg = if code == 0 {
                "任務完成".to_string()
            } else {
                format!("任務失敗 (exit {code}),記錄:{}", log_path.display())
            };
            crate::notify(&app2, &msg);
        }))
    } else {
        None
    };
    launcher::spawn(&spec, log, on_done).map_err(|e| format!("啟動失敗:{e}"))?;
    // 成功啟動過 → 視為已登入(auth 失敗時 runtime 會改回 No)
    let mut st = state.settings.lock().unwrap();
    if st.signin_state == SigninState::Unknown {
        st.signin_state = SigninState::Yes;
        let _ = settings::save(&settings::settings_path(), &st);
    }
    Ok(())
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
            "signin" => bootstrap::signin(&runner),
            "model" => bootstrap::register_model(&runner, &model),
            other => bootstrap::StepResult { ok: false, detail: format!("未知步驟 {other}") },
        }
    })
    .await
    .map_err(|e| e.to_string())?;
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
        if let Err(e) = do_launch(&app, &state, &p) {
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
