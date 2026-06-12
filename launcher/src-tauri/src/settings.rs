use serde::{Deserialize, Serialize};
use std::io::Write as _;
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
            model: crate::catalog::FALLBACKS[0].to_string(),
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
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
        if self.working_dir.is_empty() {
            home
        } else {
            let p = PathBuf::from(&self.working_dir);
            if p.is_dir() { p } else { home }
        }
    }
}

pub fn settings_path() -> PathBuf {
    dirs::config_dir().unwrap_or_else(|| PathBuf::from(".")).join("free-claude-code").join("settings.json")
}

pub fn load(path: &Path) -> Settings {
    match std::fs::read_to_string(path) {
        Err(_) => Settings::default(), // missing file – silent
        Ok(s) => match serde_json::from_str::<Settings>(&s) {
            Ok(settings) => settings,
            Err(e) => {
                eprintln!("settings: parse error ({e}), falling back to defaults");
                let _ = std::fs::copy(path, path.with_extension("json.bak"));
                Settings::default()
            }
        },
    }
}

/// Save settings atomically via a tmp file + rename.
/// Note: the fixed tmp filename means concurrent saves can lose updates
/// (single-instance app, acceptable).
pub fn save(path: &Path, s: &Settings) -> std::io::Result<()> {
    if let Some(dir) = path.parent() { std::fs::create_dir_all(dir)?; }
    let tmp = path.with_extension("json.tmp");
    let json = serde_json::to_string_pretty(s).map_err(std::io::Error::other)?;
    {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(json.as_bytes())?;
        f.sync_all()?;
    }
    std::fs::rename(&tmp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn defaults_are_per_spec() {
        let s = Settings::default();
        assert_eq!(s.hotkey, "Alt+H");
        assert_eq!(s.model, "minimax-m2.5:cloud");
        assert!(!s.cautious_mode);
        assert!(!s.background_mode);
        assert_eq!(s.working_dir, "");
        assert!(s.autostart);
        assert!(s.history.is_empty());
        assert_eq!(s.signin_state, SigninState::Unknown);
    }
    #[test]
    fn load_missing_returns_defaults_silently() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("settings.json");
        assert_eq!(load(&p), Settings::default());
    }
    #[test]
    fn load_corrupt_returns_defaults_and_creates_bak() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("settings.json");
        std::fs::write(&p, "{not json").unwrap();
        assert_eq!(load(&p), Settings::default());
        assert!(p.with_extension("json.bak").exists(), ".bak file should be created for corrupt settings");
    }
    #[test]
    fn save_then_load_roundtrip_and_partial_json_keeps_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("settings.json");
        let s = Settings { model: "qwen3-coder-next:cloud".into(), ..Default::default() };
        save(&p, &s).unwrap();
        assert_eq!(load(&p), s);
        std::fs::write(&p, r#"{"hotkey":"Ctrl+Alt+Space"}"#).unwrap();
        let partial = load(&p);
        assert_eq!(partial.hotkey, "Ctrl+Alt+Space");
        assert_eq!(partial.model, "minimax-m2.5:cloud");
    }
    #[test]
    fn overwrite_path_regression() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("settings.json");
        let s1 = Settings { model: "first-model:cloud".into(), ..Default::default() };
        save(&p, &s1).unwrap();
        let s2 = Settings { model: "second-model:cloud".into(), ..Default::default() };
        save(&p, &s2).unwrap();
        let loaded = load(&p);
        assert_eq!(loaded.model, "second-model:cloud");
        assert!(!p.with_extension("json.tmp").exists(), ".tmp file must not remain after save");
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
    #[test]
    fn effective_working_dir_empty_returns_home() {
        let s = Settings::default(); // working_dir is ""
        let result = s.effective_working_dir();
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
        assert_eq!(result, home);
    }
    #[test]
    fn effective_working_dir_existing_returns_that_path() {
        let dir = tempfile::tempdir().unwrap();
        let s = Settings { working_dir: dir.path().to_string_lossy().into_owned(), ..Default::default() };
        assert_eq!(s.effective_working_dir(), dir.path());
    }
    #[test]
    fn effective_working_dir_nonexistent_falls_back_to_home() {
        let s = Settings { working_dir: r"C:\definitely\not\here-12345".into(), ..Default::default() };
        let result = s.effective_working_dir();
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
        assert_eq!(result, home);
    }
}
