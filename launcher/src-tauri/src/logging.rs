use std::path::{Path, PathBuf};

pub fn logs_dir() -> PathBuf {
    dirs::config_dir().unwrap_or_else(|| PathBuf::from(".")).join("free-claude-code").join("logs")
}

pub fn new_run_log(dir: &Path) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
    let mut path = dir.join(format!("fcc-{stamp}.log"));
    let mut n = 1;
    while path.exists() {
        path = dir.join(format!("fcc-{stamp}-{n}.log"));
        n += 1;
    }
    std::fs::write(&path, "")?;
    Ok(path)
}

pub fn rotate(dir: &Path, keep: usize) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let mut files: Vec<PathBuf> = rd
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with("fcc-"))
        })
        .collect();
    files.sort(); // filename contains timestamp, lexicographic order = chronological order
    if files.len() > keep {
        let excess = files.len() - keep;
        for p in files.into_iter().take(excess) {
            let _ = std::fs::remove_file(p);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_log_file_and_rotates_keeping_newest() {
        let dir = tempfile::tempdir().unwrap();
        for i in 0..35 {
            let p = dir
                .path()
                .join(format!("fcc-2026061{:02}-000000.log", i % 10 + 10 * (i / 10)));
            std::fs::write(&p, "x").unwrap();
        }
        rotate(dir.path(), 30);
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 30);
        let p = new_run_log(dir.path()).unwrap();
        assert!(p.exists());
        assert!(p.file_name().unwrap().to_string_lossy().starts_with("fcc-"));
    }
}
