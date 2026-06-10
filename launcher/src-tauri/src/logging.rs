use std::path::{Path, PathBuf};

pub fn logs_dir() -> PathBuf {
    dirs::config_dir().unwrap_or_else(|| PathBuf::from(".")).join("free-claude-code").join("logs")
}

pub fn new_run_log(dir: &Path) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
    let mut n = 0u32;
    loop {
        let path = if n == 0 { dir.join(format!("fcc-{stamp}.log")) } else { dir.join(format!("fcc-{stamp}-{n}.log")) };
        match std::fs::OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(_) => return Ok(path),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => { n += 1; }
            Err(e) => return Err(e),
        }
    }
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
    fn new_run_log_collision_appends_suffix() {
        let dir = tempfile::tempdir().unwrap();
        let p1 = new_run_log(dir.path()).unwrap();
        let p2 = new_run_log(dir.path()).unwrap();
        assert_ne!(p1, p2, "both calls should return distinct paths");
        assert!(p1.exists());
        assert!(p2.exists());
        // When they share the same second, the second path must carry the -1 suffix
        let n1 = p1.file_name().unwrap().to_string_lossy().into_owned();
        let n2 = p2.file_name().unwrap().to_string_lossy().into_owned();
        // If stamps match, the second must end with "-1.log"
        let stem1 = n1.trim_end_matches(".log");
        if n2.starts_with(&format!("fcc-{}", &stem1[4..])) {
            assert!(n2.ends_with("-1.log"), "second path in same second must end with -1.log, got {n2}");
        }
    }

    #[test]
    fn creates_log_file_and_rotates_keeping_newest() {
        let dir = tempfile::tempdir().unwrap();
        for i in 0..35 {
            let p = dir
                .path()
                .join(format!("fcc-2026061{:02}-000000.log", i));
            std::fs::write(&p, "x").unwrap();
        }
        rotate(dir.path(), 30);
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 30);
        let p = new_run_log(dir.path()).unwrap();
        assert!(p.exists());
        assert!(p.file_name().unwrap().to_string_lossy().starts_with("fcc-"));
    }
}
