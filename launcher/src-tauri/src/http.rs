use std::time::Duration;

pub trait Http: Send + Sync {
    fn get(&self, url: &str, timeout: Duration) -> Result<String, String>;
    /// Stream a (possibly huge) download straight to disk — no full buffering.
    fn download_to_file(&self, url: &str, dest: &std::path::Path, timeout: Duration) -> Result<(), String>;
}

pub struct UreqHttp;

fn agent(timeout: Duration) -> ureq::Agent {
    let config = ureq::Agent::config_builder()
        .timeout_global(Some(timeout))
        .build();
    config.into()
}

impl Http for UreqHttp {
    fn get(&self, url: &str, timeout: Duration) -> Result<String, String> {
        agent(timeout)
            .get(url)
            .call()
            .map_err(|e| e.to_string())?
            .body_mut()
            .with_config()
            .limit(64 * 1024 * 1024)
            .read_to_string()
            .map_err(|e| e.to_string())
    }

    fn download_to_file(&self, url: &str, dest: &std::path::Path, timeout: Duration) -> Result<(), String> {
        let mut resp = agent(timeout)
            .get(url)
            .call()
            .map_err(|e| e.to_string())?;
        let mut file = std::fs::File::create(dest).map_err(|e| e.to_string())?;
        let mut reader = resp.body_mut().as_reader();
        std::io::copy(&mut reader, &mut file).map_err(|e| e.to_string())?;
        Ok(())
    }
}

/// Test double: url → string response body.
///
/// **Limitations:** response bodies are stored as `String` values only — binary
/// downloads are not representable. `download_to_file` writes the configured
/// string's bytes to the destination path.
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
        self.responses.get(url).cloned().unwrap_or_else(|| Err(format!("unmocked {url}")))
    }
    fn download_to_file(&self, url: &str, dest: &std::path::Path, t: Duration) -> Result<(), String> {
        let body = self.get(url, t)?;
        std::fs::write(dest, body.as_bytes()).map_err(|e| e.to_string())
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
    fn mock_http_download_to_file_writes_bytes() {
        let h = MockHttp::default().on("http://example.com/data", Ok("abc"));
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("out.bin");
        h.download_to_file("http://example.com/data", &dest, Duration::from_secs(5)).unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), b"abc");
    }

    #[test]
    fn mock_http_configured_error_propagates() {
        let h = MockHttp::default().on("http://example.com/err", Err("server down"));
        let result = h.get("http://example.com/err", Duration::from_secs(5));
        assert_eq!(result.unwrap_err(), "server down");
    }

    #[test]
    fn ureq_http_gets_body_from_local_server() {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let t = std::thread::spawn(move || {
            let (mut sock, _) = listener.accept().unwrap();
            let mut buf = [0u8; 1024];
            let _ = sock.read(&mut buf);
            let body = "hello-from-test";
            let resp = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body);
            sock.write_all(resp.as_bytes()).unwrap();
        });
        let got = UreqHttp.get(&format!("http://{addr}/"), Duration::from_secs(5)).unwrap();
        assert_eq!(got, "hello-from-test");
        t.join().unwrap();
    }

    #[test]
    fn ureq_http_download_to_file_from_local_server() {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let t = std::thread::spawn(move || {
            let (mut sock, _) = listener.accept().unwrap();
            let mut buf = [0u8; 1024];
            let _ = sock.read(&mut buf);
            let body = "streamed-content";
            let resp = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body);
            sock.write_all(resp.as_bytes()).unwrap();
        });
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("downloaded.bin");
        UreqHttp.download_to_file(&format!("http://{addr}/"), &dest, Duration::from_secs(5)).unwrap();
        assert_eq!(std::fs::read_to_string(&dest).unwrap(), "streamed-content");
        t.join().unwrap();
    }
}
