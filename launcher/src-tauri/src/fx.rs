use std::sync::atomic::{AtomicBool, Ordering};

/// True when an OS translucency effect was successfully applied to the palette.
/// Read by the status pipeline so the UI can choose translucent vs solid CSS.
pub static EFFECTS_APPLIED: AtomicBool = AtomicBool::new(false);

pub fn effects_applied() -> bool {
    EFFECTS_APPLIED.load(Ordering::Relaxed)
}

/// Frosted-glass blur for the palette window.
///
/// IMPORTANT (verified against window-vibrancy 0.7.1 source + build 22621):
/// `apply_acrylic` on Win11 22523+ takes the `DwmSetWindowAttribute(DWMSBT_…)`
/// system-backdrop path, which does NOT render behind a Tauri `transparent:true`
/// window (you just see through to the desktop, no blur, tint ignored). So we
/// use `apply_blur` instead: on every build 17763+ it uses the legacy SWCA
/// `ACCENT_ENABLE_BLURBEHIND` path, which DOES composite a blurred+tinted
/// backdrop under a transparent window — the iOS-style frosted glass we want.
/// (Drag-lag is irrelevant: the palette is fixed-size and non-draggable.)
///
/// Older than 1809 → no effect; EFFECTS_APPLIED=false makes the UI fall back to
/// a solid background so a transparent window isn't fully see-through.
pub fn apply_palette_effects(window: &tauri::WebviewWindow) {
    let build = windows_version::OsVersion::current().build;
    let result = if build >= 17763 {
        // Dark, semi-transparent tint over the OS blur (R, G, B, A).
        window_vibrancy::apply_blur(window, Some((18, 18, 26, 140)))
    } else {
        Err(window_vibrancy::Error::UnsupportedPlatformVersion(
            "blur requires Windows 10 v1809 (build 17763) or newer",
        ))
    };
    EFFECTS_APPLIED.store(result.is_ok(), Ordering::Relaxed);
}
