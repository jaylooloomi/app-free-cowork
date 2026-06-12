use std::sync::atomic::{AtomicBool, Ordering};

/// True when an OS translucency effect was successfully applied to the palette.
/// Read by the status pipeline so the UI can choose translucent vs solid CSS.
pub static EFFECTS_APPLIED: AtomicBool = AtomicBool::new(false);

pub fn effects_applied() -> bool {
    EFFECTS_APPLIED.load(Ordering::Relaxed)
}

/// Version-aware acrylic for the palette window.
///
/// - Win11 22523+:系統 backdrop acrylic(DWMSBT_TRANSIENTWINDOW;tint 參數被忽略)。
/// - Win10 1809(build 17763)..Win11:SWCA acrylic 加深色 tint
///   (已知拖曳會卡頓,但 palette 固定大小、不可拖曳,無影響)。
/// - 更舊:不套效果——透明視窗若無效果會整片透視,
///   因此以 EFFECTS_APPLIED=false 讓前端改用純色背景。
pub fn apply_palette_effects(window: &tauri::WebviewWindow) {
    let build = windows_version::OsVersion::current().build;
    let result = if build >= 17763 {
        window_vibrancy::apply_acrylic(window, Some((20, 20, 28, 160)))
    } else {
        Err(window_vibrancy::Error::UnsupportedPlatformVersion(
            "acrylic requires Windows 10 v1809 (build 17763) or newer",
        ))
    };
    EFFECTS_APPLIED.store(result.is_ok(), Ordering::Relaxed);
}
