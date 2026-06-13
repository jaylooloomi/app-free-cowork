pub mod bootstrap;
pub mod catalog;
pub mod command;
pub mod doctor;
pub mod fx;
pub mod http;
pub mod i18n;
pub mod ipc;
pub mod launcher;
pub mod logging;
pub mod settings;
pub mod trust;
pub mod version;
pub mod voice;

use ipc::AppState;
use tauri::{AppHandle, Emitter as _, Manager};
use tauri_plugin_autostart::ManagerExt as _;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

/// 本 app 的 AppUserModelID,必須與安裝捷徑、登錄機碼三者一致,Windows 才能
/// 正確解析 toast 左上角的歸屬 icon。
const APP_USER_MODEL_ID: &str = "com.jaylooloomi.free-claude-code";

/// 解析隨安裝包帶入的 icon 檔(優先 .ico,退回 128x128.png)。
fn toast_icon_path(app: &AppHandle) -> Option<std::path::PathBuf> {
    let dir = app.path().resource_dir().ok()?;
    let ico = dir.join("icons").join("icon.ico");
    if ico.exists() {
        return Some(ico);
    }
    let png = dir.join("icons").join("128x128.png");
    png.exists().then_some(png)
}

/// resource_dir() 在 Windows 會回傳帶 `\\?\` 擴充長度前綴的路徑;Windows 殼層
/// /通知的 icon 解析不吃這種前綴,寫進 IconUri / .icon() 前先去掉它。
fn clean_path_string(p: &std::path::Path) -> String {
    let s = p.to_string_lossy();
    s.strip_prefix(r"\\?\").unwrap_or(&s).to_string()
}

pub fn notify(app: &AppHandle, body: &str) {
    use tauri_plugin_notification::NotificationExt;
    let mut builder = app.notification().builder().title("FreeCowork").body(body);
    // .icon() 只填 toast 內文的 appLogoOverride;左上角歸屬 icon 由 AUMID 登錄
    // 機碼決定(見 register_toast_icon)。兩者都設,通知才完整顯示本 app 圖示。
    if let Some(icon) = toast_icon_path(app) {
        builder = builder.icon(clean_path_string(&icon));
    }
    let _ = builder.show();
}

/// 讓本行程明確採用與捷徑/登錄一致的 AUMID;否則 Windows 可能用預設值,
/// 與登錄機碼對不上,toast 就找不到 icon。必須在顯示任何通知前呼叫。
#[cfg(windows)]
fn set_app_user_model_id() {
    use windows::core::w;
    use windows::Win32::UI::Shell::SetCurrentProcessExplicitAppUserModelID;
    unsafe {
        let _ = SetCurrentProcessExplicitAppUserModelID(w!("com.jaylooloomi.free-claude-code"));
    }
}

/// 把 AUMID 綁定到 app icon:寫 HKCU\Software\Classes\AppUserModelId\<id>
/// 的 DisplayName / IconUri。這是 Windows 為「未封裝(NSIS)應用」決定 toast
/// 左上角 icon 的唯一來源。冪等(每次啟動重寫),可直接修好已安裝的機器、
/// 重裝也存活。失敗只略過(通知仍會顯示,只是沒左上角 icon)。
fn register_toast_icon(app: &AppHandle) {
    let Some(icon) = toast_icon_path(app) else { return };
    let hkcu = winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER);
    let path = format!(r"Software\Classes\AppUserModelId\{APP_USER_MODEL_ID}");
    if let Ok((key, _)) = hkcu.create_subkey(&path) {
        let _ = key.set_value("DisplayName", &"FreeCowork".to_string());
        let _ = key.set_value("IconUri", &clean_path_string(&icon));
        let _ = key.set_value("IconBackgroundColor", &"FF2D2D2D".to_string());
    }
}

pub fn register_hotkey(app: &AppHandle, hotkey: &str) -> Result<(), String> {
    let gs = app.global_shortcut();
    let _ = gs.unregister_all();
    gs.on_shortcut(hotkey, |app, _shortcut, event| {
        if event.state() == ShortcutState::Pressed {
            show_palette_centered(app);
        }
    })
    .map_err(|e| format!("快捷鍵註冊失敗({hotkey}):{e}"))
}

pub fn sync_autostart(app: &AppHandle, enabled: bool) {
    let am = app.autolaunch();
    let _ = if enabled { am.enable() } else { am.disable() };
}

fn show_palette_centered(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("palette") {
        // 已顯示 → 再按一次快捷鍵 = 關閉面板(標準 toggle 行為)。
        // 語音輸入改由面板上的麥克風鈕觸發。
        if w.is_visible().unwrap_or(false) {
            let _ = w.hide();
            return;
        }
        // Lazy catalog refresh: recover from offline-at-boot
        if app.state::<ipc::AppState>().catalog_cache.lock().unwrap().is_empty() {
            refresh_catalog(app.clone());
        }
        if let Ok(Some(monitor)) = w.current_monitor() {
            let ms = monitor.size();
            let mp = monitor.position();
            let ws = w.outer_size().unwrap_or(tauri::PhysicalSize { width: 640, height: 168 });
            let x = mp.x + ((ms.width.saturating_sub(ws.width)) / 2) as i32;
            let y = mp.y + (ms.height / 4) as i32;
            let _ = w.set_position(tauri::PhysicalPosition { x, y });
        }
        let _ = w.show();
        let _ = w.set_focus();
        let _ = w.emit("palette-shown", ());
    }
}

fn handle_argv(app: &AppHandle, argv: &[String]) {
    if let Some(i) = argv.iter().position(|a| a == "--run") {
        if let Some(prompt) = argv.get(i + 1) {
            let handle = app.clone();
            let prompt = prompt.clone();
            tauri::async_runtime::spawn(async move {
                let state = handle.state::<AppState>();
                if let Err(e) = ipc::submit_prompt(handle.clone(), state, prompt).await {
                    notify(&handle, &e);
                }
            });
            return;
        }
    }
    if argv.iter().any(|a| a == "--show-palette") {
        show_palette_centered(app);
    }
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

fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    use tauri::menu::{Menu, MenuItem};
    use tauri::tray::TrayIconBuilder;
    let open = MenuItem::with_id(app, "open", "開啟輸入面板", true, None::<&str>)?;
    let settings_item = MenuItem::with_id(app, "settings", "設定", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "結束", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &settings_item, &quit])?;
    TrayIconBuilder::with_id("main")
        .icon(
            app.default_window_icon()
                .ok_or_else(|| tauri::Error::Io(std::io::Error::other("missing default window icon")))?
                .clone(),
        )
        .menu(&menu)
        .tooltip("FreeCowork")
        .on_menu_event(|app, e| match e.id.as_ref() {
            "open" => show_palette_centered(app),
            "settings" => ipc::show_window(app, "settings"),
            "quit" => app.exit(0),
            _ => {}
        })
        .build(app)?;
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // toast 左上角 icon 靠 AUMID 解析 → 行程一啟動就採用固定 AUMID(早於任何通知)。
    #[cfg(windows)]
    set_app_user_model_id();
    let loaded = settings::load(&settings::settings_path());
    tauri::Builder::default()
        // single-instance 必須最先註冊(Tauri 文件要求)
        .plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
            handle_argv(app, &argv);
            // 使用者直接再點一次程式(無參數)→ 視為「叫出輸入面板」
            // (僅限第二份實例;開機那份的 handle_argv 不受影響)
            if !argv.iter().any(|a| a == "--run" || a == "--show-palette") {
                show_palette_centered(app);
            }
        }))
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .manage(AppState::new(loaded.clone()))
        .invoke_handler(tauri::generate_handler![
            ipc::get_settings,
            ipc::save_settings,
            ipc::get_status,
            ipc::get_history,
            ipc::submit_prompt,
            ipc::wizard_plan,
            ipc::wizard_run,
            ipc::wizard_done,
            ipc::list_cloud_models,
            ipc::open_logs,
            ipc::hide_palette,
            ipc::open_settings_window,
            ipc::queue_list,
            ipc::queue_cancel,
            ipc::dismiss_completed,
            ipc::task_stop,
            ipc::list_models_ui,
            ipc::set_model,
            ipc::scan_models,
            ipc::open_url,
            ipc::start_voice_input,
            ipc::effects_applied,
            ipc::save_pasted_image,
            ipc::capture_screenshot
        ])
        .on_window_event(|window, event| {
            // 三個視窗都只隱藏、永不關閉 — 否則 X 會銷毀視窗導致 app 結束
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .setup(move |app| {
            if let Some(w) = app.get_webview_window("palette") {
                fx::apply_palette_effects(&w);
            }
            let handle = app.handle().clone();
            if let Err(e) = register_hotkey(&handle, &loaded.hotkey) {
                notify(&handle, &e);
                ipc::show_window(&handle, "settings");
            }
            sync_autostart(&handle, loaded.autostart);
            // 綁定 AUMID→icon 登錄機碼(修好 toast 左上角 icon;冪等)
            register_toast_icon(&handle);
            refresh_catalog(handle.clone());
            ipc::refresh_plan(handle.clone());
            build_tray(&handle)?;
            let argv: Vec<String> = std::env::args().collect();
            handle_argv(&handle, &argv);
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
