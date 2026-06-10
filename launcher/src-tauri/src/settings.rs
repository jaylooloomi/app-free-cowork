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
