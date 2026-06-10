use std::time::Duration;

pub trait Http: Send + Sync {
    fn get(&self, url: &str, timeout: Duration) -> Result<String, String>;
    fn get_bytes(&self, url: &str, timeout: Duration) -> Result<Vec<u8>, String>;
}

pub struct UreqHttp;

impl Http for UreqHttp {
    fn get(&self, url: &str, timeout: Duration) -> Result<String, String> {
        let config = ureq::Agent::config_builder()
            .timeout_global(Some(timeout))
            .build();
        let agent: ureq::Agent = config.into();
        agent
            .get(url)
            .call()
            .map_err(|e| e.to_string())?
            .body_mut()
            .read_to_string()
            .map_err(|e| e.to_string())
    }

    fn get_bytes(&self, url: &str, timeout: Duration) -> Result<Vec<u8>, String> {
        let config = ureq::Agent::config_builder()
            .timeout_global(Some(timeout))
            .build();
        let agent: ureq::Agent = config.into();
        agent
            .get(url)
            .call()
            .map_err(|e| e.to_string())?
            .body_mut()
            .read_to_vec()
            .map_err(|e| e.to_string())
    }
}

/// Test double: url → response.
#[derive(Default)]
pub struct MockHttp { pub responses: std::collections::HashMap<String, Result<String, String>> }
impl MockHttp {
    pub fn on(mut self, url: &str, resp: Result<&str, &str>) -> Self {
        self.responses.insert(url.into(), resp.map(String::from).map_err(String::from));
        self
    }
}
impl Http for MockHttp {
    fn get(&self, url: &str, _t: Duration) -> Result<String, String> {
        self.responses.get(url).cloned().unwrap_or(Err(format!("unmocked {url}")))
    }
    fn get_bytes(&self, url: &str, t: Duration) -> Result<Vec<u8>, String> {
        self.get(url, t).map(|s| s.into_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mock_http_returns_configured_response() {
        let h = MockHttp::default().on("http://example.com/ok", Ok("hello"));
        let result = h.get("http://example.com/ok", Duration::from_secs(5));
        assert_eq!(result.unwrap(), "hello");
    }

    #[test]
    fn mock_http_returns_error_for_unconfigured_url() {
        let h = MockHttp::default();
        let result = h.get("http://example.com/missing", Duration::from_secs(5));
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("unmocked"));
    }

    #[test]
    fn mock_http_get_bytes_converts_string_to_bytes() {
        let h = MockHttp::default().on("http://example.com/data", Ok("abc"));
        let bytes = h.get_bytes("http://example.com/data", Duration::from_secs(5)).unwrap();
        assert_eq!(bytes, b"abc");
    }

    #[test]
    fn mock_http_configured_error_propagates() {
        let h = MockHttp::default().on("http://example.com/err", Err("server down"));
        let result = h.get("http://example.com/err", Duration::from_secs(5));
        assert_eq!(result.unwrap_err(), "server down");
    }
}
