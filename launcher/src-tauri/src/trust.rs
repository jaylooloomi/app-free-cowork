//! Pre-accepts Claude Code's per-directory workspace trust.
//!
//! On first use of a directory, Claude Code shows a "Is this a project you
//! trust?" dialog and waits for Enter — friction that breaks the "type one
//! line, it just runs" promise for a fresh install. Trust is recorded in
//! `~/.claude.json` under `projects[<path>].hasTrustDialogAccepted` (schema
//! verified live 2026-06-13). Background (`-p`) launches auto-trust; foreground
//! interactive launches do not, so we pre-seed the flag ourselves before a
//! non-cautious foreground launch.
//!
//! Only invoked when NOT in cautious mode: cautious mode deliberately keeps the
//! trust dialog as a safety gate.

use std::path::{Path, PathBuf};

fn claude_json_path() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".claude.json"))
}

/// Claude Code keys projects by a forward-slash path (observed) but Windows
/// `Path::to_string_lossy` yields backslashes — seed BOTH so the key matches
/// whatever form Claude Code resolves the cwd to.
pub fn path_key_variants(cwd: &Path) -> Vec<String> {
    let raw = cwd.to_string_lossy().into_owned();
    let fwd = raw.replace('\\', "/");
    let mut v = vec![raw];
    if !v.contains(&fwd) {
        v.push(fwd);
    }
    v
}

/// Ensure `projects[key].hasTrustDialogAccepted == true` for every key, merging
/// into an existing object so other Claude Code state is preserved. Creates the
/// `projects` map and per-path entries (matching the real schema) when absent.
/// Returns true if anything changed.
pub fn merge_trust(root: &mut serde_json::Value, keys: &[String]) -> bool {
    use serde_json::{Map, Value};
    if !root.is_object() {
        *root = Value::Object(Map::new());
    }
    let obj = root.as_object_mut().unwrap();
    let projects = obj
        .entry("projects")
        .or_insert_with(|| Value::Object(Map::new()));
    if !projects.is_object() {
        *projects = Value::Object(Map::new());
    }
    let projects = projects.as_object_mut().unwrap();
    let mut changed = false;
    for key in keys {
        let entry = projects
            .entry(key.clone())
            .or_insert_with(|| {
                changed = true;
                serde_json::json!({
                    "allowedTools": [],
                    "hasTrustDialogAccepted": true,
                    "projectOnboardingSeenCount": 0
                })
            });
        if let Some(e) = entry.as_object_mut() {
            if e.get("hasTrustDialogAccepted") != Some(&Value::Bool(true)) {
                e.insert("hasTrustDialogAccepted".into(), Value::Bool(true));
                changed = true;
            }
        }
    }
    changed
}

/// Best-effort: pre-accept workspace trust for `cwd`. Never panics; a failure
/// just means the user sees the one-time dialog (no worse than before).
pub fn ensure_trusted(cwd: &Path) {
    let Some(path) = claude_json_path() else { return };
    let mut root: serde_json::Value = std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| serde_json::json!({}));
    let keys = path_key_variants(cwd);
    if !merge_trust(&mut root, &keys) {
        return; // already trusted — don't rewrite Claude Code's file needlessly
    }
    if let Ok(text) = serde_json::to_string_pretty(&root) {
        // Atomic-ish: write tmp then rename so a concurrent Claude Code read
        // never sees a half-written file.
        let tmp = path.with_extension("json.fcc-tmp");
        if std::fs::write(&tmp, &text).is_ok() {
            let _ = std::fs::rename(&tmp, &path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn path_variants_cover_forward_and_back_slash() {
        let v = path_key_variants(&PathBuf::from(r"C:\Users\王鼎傑"));
        assert!(v.contains(&r"C:\Users\王鼎傑".to_string()));
        assert!(v.contains(&"C:/Users/王鼎傑".to_string()));
    }

    #[test]
    fn forward_slash_path_yields_single_variant() {
        let v = path_key_variants(&PathBuf::from("C:/x"));
        assert_eq!(v, vec!["C:/x".to_string()]);
    }

    #[test]
    fn merge_creates_projects_and_sets_trust() {
        let mut root = serde_json::json!({});
        let changed = merge_trust(&mut root, &["C:/Users/me".into()]);
        assert!(changed);
        assert_eq!(
            root["projects"]["C:/Users/me"]["hasTrustDialogAccepted"],
            serde_json::Value::Bool(true)
        );
    }

    #[test]
    fn merge_flips_existing_false_and_preserves_siblings() {
        let mut root = serde_json::json!({
            "numStartups": 23,
            "projects": {
                "C:/Users/me": { "hasTrustDialogAccepted": false, "allowedTools": ["Bash"] }
            }
        });
        let changed = merge_trust(&mut root, &["C:/Users/me".into()]);
        assert!(changed);
        assert_eq!(root["projects"]["C:/Users/me"]["hasTrustDialogAccepted"], serde_json::Value::Bool(true));
        // sibling fields untouched
        assert_eq!(root["numStartups"], 23);
        assert_eq!(root["projects"]["C:/Users/me"]["allowedTools"][0], "Bash");
    }

    #[test]
    fn merge_is_idempotent_when_already_true() {
        let mut root = serde_json::json!({
            "projects": { "C:/x": { "hasTrustDialogAccepted": true } }
        });
        assert!(!merge_trust(&mut root, &["C:/x".into()]), "no change when already trusted");
    }
}
