pub mod announce;
pub mod bootstrap;
pub mod schedule;
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

/// 回傳「目前焦點視窗」所在螢幕的實體座標原點與大小(虛擬桌面座標系)。
/// 必須在 palette 的 show()/set_focus() 之前呼叫 — 此時前景視窗仍是使用者
/// 剛才在操作的那個,於是面板會跟著出現在他正在用的螢幕上。
/// 無前景視窗或 API 失敗回 None,呼叫端 fallback。
#[cfg(windows)]
fn focused_monitor_rect() -> Option<(tauri::PhysicalPosition<i32>, tauri::PhysicalSize<u32>)> {
    use windows::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST,
    };
    use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.0.is_null() {
            return None;
        }
        let hmon = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
        if hmon.0.is_null() {
            return None;
        }
        let mut mi = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if !GetMonitorInfoW(hmon, &mut mi).as_bool() {
            return None;
        }
        let r = mi.rcMonitor;
        Some((
            tauri::PhysicalPosition { x: r.left, y: r.top },
            tauri::PhysicalSize {
                width: (r.right - r.left).max(0) as u32,
                height: (r.bottom - r.top).max(0) as u32,
            },
        ))
    }
}

#[cfg(not(windows))]
fn focused_monitor_rect() -> Option<(tauri::PhysicalPosition<i32>, tauri::PhysicalSize<u32>)> {
    None
}

/// 純函式:給定目標螢幕的原點、大小與視窗大小,算出讓視窗在該螢幕水平置中、
/// 垂直落在上方 1/4 處的左上角座標。抽出成純函式以便單元測試 — 多螢幕定位的
/// 核心數學就在這裡(座標都相對於「所選螢幕」的原點)。
fn centered_position(
    monitor_pos: tauri::PhysicalPosition<i32>,
    monitor_size: tauri::PhysicalSize<u32>,
    window_size: tauri::PhysicalSize<u32>,
) -> tauri::PhysicalPosition<i32> {
    let x = monitor_pos.x + ((monitor_size.width.saturating_sub(window_size.width)) / 2) as i32;
    let y = monitor_pos.y + (monitor_size.height / 4) as i32;
    tauri::PhysicalPosition { x, y }
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
        // 優先用「目前焦點視窗所在螢幕」,讓面板跨多螢幕跟隨使用者;
        // 偵測失敗(非 Windows、無前景視窗、API 錯誤)時 fallback 回 palette
        // 自己上次所在的螢幕(舊行為),確保絕不比現況更糟。
        let rect = focused_monitor_rect().or_else(|| {
            w.current_monitor()
                .ok()
                .flatten()
                .map(|m| (*m.position(), *m.size()))
        });
        if let Some((mp, ms)) = rect {
            let ws = w.outer_size().unwrap_or(tauri::PhysicalSize { width: 640, height: 168 });
            let _ = w.set_position(centered_position(mp, ms, ws));
        }
        let _ = w.show();
        let _ = w.set_focus();
        let _ = w.emit("palette-shown", ());
    }
}

/// 待播報的摘要文字。Rust 在 show 前存入,前端掛載/收到通知時用 take_announce
/// 主動拉取 —— 避免「emit 早於 webview 監聽就緒」的 race(首次顯示尤其會漏接)。
static PENDING_ANNOUNCE: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

/// 顯示任務完成播報 overlay:定位到焦點螢幕下方中央,套上 WS_EX_NOACTIVATE
/// (Tauri 的 focus flag 在 Windows 不可靠,用這個才能保證 show 不搶使用者焦點、
/// 不打斷打字),點擊穿透,顯示但不 set_focus,並通知前端去拉取待播文字。
pub fn show_announcer(app: &AppHandle, text: &str) {
    let Some(w) = app.get_webview_window("announcer") else {
        return;
    };
    // 先存待播文字,再顯示;前端就緒後會 take_announce 拉取(可靠,不怕事件 race)。
    if let Ok(mut g) = PENDING_ANNOUNCE.lock() {
        *g = Some(text.to_string());
    }
    let rect = focused_monitor_rect().or_else(|| {
        w.current_monitor()
            .ok()
            .flatten()
            .map(|m| (*m.position(), *m.size()))
    });
    if let Some((mp, ms)) = rect {
        let ws = w
            .outer_size()
            .unwrap_or(tauri::PhysicalSize { width: 600, height: 220 });
        // margin 160:離工作列遠一點、往上(使用者要求位置上移)。
        let _ = w.set_position(announce::bottom_centered_position(mp, ms, ws, 160));
    }
    #[cfg(windows)]
    if let Ok(hwnd) = w.hwnd() {
        use windows::Win32::Foundation::HWND;
        use windows::Win32::UI::WindowsAndMessaging::{
            GetWindowLongPtrW, SetWindowLongPtrW, GWL_EXSTYLE, WS_EX_NOACTIVATE,
        };
        // 跨 windows-crate 版本安全地重建 HWND(Tauri 的 windows 版本未必等於本 crate)。
        let h = HWND(hwnd.0 as _);
        unsafe {
            let ex = GetWindowLongPtrW(h, GWL_EXSTYLE);
            SetWindowLongPtrW(h, GWL_EXSTYLE, ex | (WS_EX_NOACTIVATE.0 as isize));
        }
    }
    // 關閉點擊穿透:讓停止鈕/點面板可被點到。WS_EX_NOACTIVATE 已套,點擊不會搶焦點。
    let _ = w.set_ignore_cursor_events(false);
    let _ = w.show();
    // 故意不呼叫 set_focus():保持使用者原本的前景視窗持有鍵盤焦點。
    // 通知前端「有新播報可拉」;即使此事件因 race 漏接,前端 onMount 的拉取也會補上。
    let _ = w.emit("announce", ());
}

/// 前端拉取待播文字(並清空)。回 None = 沒有待播。
#[tauri::command]
fn take_announce() -> Option<String> {
    PENDING_ANNOUNCE.lock().ok().and_then(|mut g| g.take())
}

/// 播報唸完/淡出後,前端呼叫此命令把 overlay 藏起來(下次播報再 show)。
#[tauri::command]
fn announcer_done(app: AppHandle) {
    if let Some(w) = app.get_webview_window("announcer") {
        let _ = w.hide();
    }
}

/// 用 DWM 把視窗(含 OS 模糊底)裁成圓角。沒這個的話,apply_blur 的模糊底會填滿
/// 整個方形視窗,在內容外緣露出方塊("黑方塊")。DWMWCP_ROUND 在 DWM 合成層裁切,
/// 連模糊底一起裁圓。半徑是 Win11 系統固定值(約 8px)。
#[cfg(windows)]
fn round_window_corners(window: &tauri::WebviewWindow) {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::Graphics::Dwm::{
        DwmSetWindowAttribute, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND,
        DWM_WINDOW_CORNER_PREFERENCE,
    };
    let Ok(raw) = window.hwnd() else {
        return;
    };
    // 跨 windows-crate 版本安全地重建 HWND。
    let hwnd = HWND(raw.0 as _);
    let pref: DWM_WINDOW_CORNER_PREFERENCE = DWMWCP_ROUND;
    unsafe {
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            &pref as *const _ as *const core::ffi::c_void,
            std::mem::size_of::<DWM_WINDOW_CORNER_PREFERENCE>() as u32,
        );
    }
}

fn handle_argv(app: &AppHandle, argv: &[String]) {
    if let Some(i) = argv.iter().position(|a| a == "--run") {
        if let Some(prompt) = argv.get(i + 1) {
            let handle = app.clone();
            let prompt = prompt.clone();
            tauri::async_runtime::spawn(async move {
                let state = handle.state::<AppState>();
                if let Err(e) = ipc::submit_prompt(handle.clone(), state, prompt, None).await {
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
            ipc::hide_settings,
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
            ipc::capture_screenshot,
            ipc::pick_folder,
            announcer_done,
            take_announce
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
            // 播報 overlay:深色真毛玻璃(apply_blur 真模糊桌面)+ DWM 圓角
            // (把整個方形模糊底裁成圓角,消除露在內容外的「黑方塊」)。
            if let Some(w) = app.get_webview_window("announcer") {
                fx::apply_announcer_effects(&w);
                #[cfg(windows)]
                round_window_corners(&w);
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

#[cfg(test)]
mod tests {
    use super::centered_position;
    use tauri::{PhysicalPosition, PhysicalSize};

    #[test]
    fn centers_horizontally_and_quarter_from_top_on_primary_monitor() {
        // 主螢幕原點 (0,0)、1920x1080,palette 640x168
        let pos = centered_position(
            PhysicalPosition { x: 0, y: 0 },
            PhysicalSize { width: 1920, height: 1080 },
            PhysicalSize { width: 640, height: 168 },
        );
        assert_eq!(pos.x, (1920 - 640) / 2); // 640
        assert_eq!(pos.y, 1080 / 4); // 270
    }

    #[test]
    fn offsets_by_monitor_origin_for_secondary_monitor() {
        // 右側第二螢幕,原點 x=1920(本功能的重點:座標相對所選螢幕原點)
        let pos = centered_position(
            PhysicalPosition { x: 1920, y: 0 },
            PhysicalSize { width: 2560, height: 1440 },
            PhysicalSize { width: 640, height: 168 },
        );
        assert_eq!(pos.x, 1920 + (2560 - 640) / 2); // 2880
        assert_eq!(pos.y, 1440 / 4); // 360
    }

    #[test]
    fn handles_monitor_with_negative_origin() {
        // 左側螢幕原點為負(Windows 虛擬桌面常見)
        let pos = centered_position(
            PhysicalPosition { x: -1920, y: -120 },
            PhysicalSize { width: 1920, height: 1080 },
            PhysicalSize { width: 640, height: 168 },
        );
        assert_eq!(pos.x, -1920 + (1920 - 640) / 2); // -1280
        assert_eq!(pos.y, -120 + 1080 / 4); // 150
    }

    #[test]
    fn clamps_when_window_wider_than_monitor() {
        // 視窗比螢幕寬 → saturating_sub 防 underflow,x 落在螢幕原點而非更左
        let pos = centered_position(
            PhysicalPosition { x: 100, y: 50 },
            PhysicalSize { width: 320, height: 240 },
            PhysicalSize { width: 640, height: 168 },
        );
        assert_eq!(pos.x, 100);
        assert_eq!(pos.y, 50 + 240 / 4); // 110
    }
}
