use std::time::Duration;

pub trait Http: Send + Sync {
    fn get(&self, url: &str, timeout: Duration) -> Result<String, String>;
    /// Stream a (possibly huge) download straight to disk — no full buffering.
    fn download_to_file(&self, url: &str, dest: &std::path::Path, timeout: Duration) -> Result<(), String>;
    /// POST `body` as application/json, returning the response body.
    fn post(&self, url: &str, body: &str, timeout: Duration) -> Result<String, String>;
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

    fn post(&self, url: &str, body: &str, timeout: Duration) -> Result<String, String> {
        agent(timeout)
            .post(url)
            .header("Content-Type", "application/json")
            .send(body)
            .map_err(|e| e.to_string())?
            .body_mut()
            .read_to_string()
            .map_err(|e| e.to_string())
    }
}

/// Test double: url → string response body.
///
/// **Limitations:** response bodies are stored as `String` values only — binary
/// downloads are not representable. `download_to_file` writes the configured
/// string's bytes to the destination path.
///
/// Use `failing_first(url, n)` to make the first `n` calls to `get(url)` return
/// `Err("simulated failure")` before falling through to the configured response.
#[derive(Default)]
pub struct MockHttp {
    pub responses: std::collections::HashMap<String, Result<String, String>>,
    /// url → remaining failures before the configured response is returned
    pub fail_first: std::sync::Mutex<std::collections::HashMap<String, u32>>,
    /// url → POST response body (configure with `on_post`)
    pub post_responses: std::collections::HashMap<String, Result<String, String>>,
    /// every `post` call recorded as (url, body)
    pub posts: std::sync::Mutex<Vec<(String, String)>>,
}
impl MockHttp {
    pub fn on(mut self, url: &str, resp: Result<&str, &str>) -> Self {
        self.responses.insert(url.into(), resp.map(String::from).map_err(String::from));
        self
    }
    pub fn on_post(mut self, url: &str, resp: Result<&str, &str>) -> Self {
        self.post_responses.insert(url.into(), resp.map(String::from).map_err(String::from));
        self
    }
    pub fn failing_first(self, url: &str, times: u32) -> Self {
        self.fail_first.lock().unwrap().insert(url.into(), times);
        self
    }
}
impl Http for MockHttp {
    fn get(&self, url: &str, _t: Duration) -> Result<String, String> {
        {
            let mut ff = self.fail_first.lock().unwrap();
            if let Some(rem) = ff.get_mut(url) {
                if *rem > 0 {
                    *rem -= 1;
                    return Err("simulated failure".into());
                }
            }
        }
        self.responses.get(url).cloned().unwrap_or_else(|| Err(format!("unmocked {url}")))
    }
    fn download_to_file(&self, url: &str, dest: &std::path::Path, t: Duration) -> Result<(), String> {
        let body = self.get(url, t)?;
        std::fs::write(dest, body.as_bytes()).map_err(|e| e.to_string())
    }
    fn post(&self, url: &str, body: &str, _t: Duration) -> Result<String, String> {
        self.posts.lock().unwrap().push((url.to_string(), body.to_string()));
        self.post_responses.get(url).cloned().unwrap_or_else(|| Err(format!("unmocked POST {url}")))
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
    fn mock_http_post_returns_configured_response_and_records_call() {
        let h = MockHttp::default().on_post("http://example.com/api/me", Ok(r#"{"plan":"free"}"#));
        let result = h.post("http://example.com/api/me", "{}", Duration::from_secs(5));
        assert_eq!(result.unwrap(), r#"{"plan":"free"}"#);
        let posts = h.posts.lock().unwrap();
        assert_eq!(posts.as_slice(), &[("http://example.com/api/me".to_string(), "{}".to_string())]);
    }

    #[test]
    fn mock_http_post_unmocked_url_errors() {
        let h = MockHttp::default();
        let err = h.post("http://example.com/nope", "{}", Duration::from_secs(5)).unwrap_err();
        assert!(err.contains("unmocked POST"));
        assert_eq!(h.posts.lock().unwrap().len(), 1, "failed posts are still recorded");
    }

    #[test]
    fn ureq_http_posts_json_body_and_reads_response() {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let t = std::thread::spawn(move || {
            let (mut sock, _) = listener.accept().unwrap();
            // headers 與 body 可能分批送達 — 讀到完整請求(空行 + "{}" body)為止
            let mut raw = Vec::new();
            let mut buf = [0u8; 1024];
            loop {
                let n = sock.read(&mut buf).unwrap();
                if n == 0 {
                    break;
                }
                raw.extend_from_slice(&buf[..n]);
                let s = String::from_utf8_lossy(&raw);
                if s.contains("\r\n\r\n") && s.ends_with("{}") {
                    break;
                }
            }
            let req = String::from_utf8_lossy(&raw).into_owned();
            let body = r#"{"plan":"free"}"#;
            let resp = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", body.len(), body);
            sock.write_all(resp.as_bytes()).unwrap();
            req
        });
        let got = UreqHttp.post(&format!("http://{addr}/api/me"), "{}", Duration::from_secs(5)).unwrap();
        assert_eq!(got, r#"{"plan":"free"}"#);
        let req = t.join().unwrap();
        assert!(req.starts_with("POST /api/me"), "expected POST request line; got: {req}");
        assert!(req.to_lowercase().contains("content-type: application/json"), "missing json content-type; got: {req}");
        assert!(req.ends_with("{}"), "request body should be {{}}; got: {req}");
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
