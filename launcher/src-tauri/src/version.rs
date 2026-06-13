use semver::Version;

pub fn min_ollama() -> Version { Version::new(0, 15, 6) }

pub fn parse_ollama_version(stdout: &str) -> Option<Version> {
    stdout.lines().find_map(|line| {
        let token = line.trim().strip_prefix("ollama version is ")?.trim();
        Version::parse(token.split_whitespace().next()?).ok()
    })
}

pub fn meets_min(v: &Version) -> bool { *v >= min_ollama() }

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_ollama_version_line() {
        assert_eq!(parse_ollama_version("ollama version is 0.30.6").unwrap().to_string(), "0.30.6");
        assert_eq!(parse_ollama_version("ollama version is 0.15.6\nWarning: foo").unwrap().to_string(), "0.15.6");
        assert!(parse_ollama_version("garbage").is_none());
    }
    #[test]
    fn min_version_gate() {
        assert!(meets_min(&parse_ollama_version("ollama version is 0.15.6").unwrap()));
        assert!(meets_min(&parse_ollama_version("ollama version is 0.30.0").unwrap()));
        assert!(!meets_min(&parse_ollama_version("ollama version is 0.15.5").unwrap()));
        assert!(!meets_min(&parse_ollama_version("ollama version is 0.14.9").unwrap()));
    }
}
