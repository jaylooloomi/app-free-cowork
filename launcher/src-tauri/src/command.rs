use std::io;
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct CmdOutput { pub code: i32, pub stdout: String, pub stderr: String }
impl CmdOutput { pub fn ok(&self) -> bool { self.code == 0 } }

pub trait Runner: Send + Sync {
    /// Run synchronously collecting output; kill on timeout returning io::Error.
    fn run(&self, program: &str, args: &[&str], timeout: Duration) -> io::Result<CmdOutput>;
    /// Fire-and-forget with hidden window (used for `ollama serve`).
    fn spawn_detached(&self, program: &str, args: &[&str]) -> io::Result<()>;
}

pub struct SystemRunner;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

impl Runner for SystemRunner {
    fn run(&self, program: &str, args: &[&str], timeout: Duration) -> io::Result<CmdOutput> {
        use std::io::Read;
        use std::os::windows::process::CommandExt;
        use std::process::{Command, Stdio};
        use wait_timeout::ChildExt;
        let mut child = Command::new(program)
            .args(args)
            .creation_flags(CREATE_NO_WINDOW)
            .stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped())
            .spawn()?;
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        let t_out = std::thread::spawn(move || {
            let mut v = Vec::new();
            if let Some(mut s) = stdout { let _ = s.read_to_end(&mut v); }
            v
        });
        let t_err = std::thread::spawn(move || {
            let mut v = Vec::new();
            if let Some(mut s) = stderr { let _ = s.read_to_end(&mut v); }
            v
        });
        let status = match child.wait_timeout(timeout)? {
            Some(s) => s,
            None => {
                let _ = child.kill();
                let _ = child.wait(); // reap; also closes pipes so reader threads finish
                return Err(io::Error::new(io::ErrorKind::TimedOut, format!("{program} timed out")));
            }
        };
        let out = String::from_utf8_lossy(&t_out.join().unwrap_or_default()).into_owned();
        let err = String::from_utf8_lossy(&t_err.join().unwrap_or_default()).into_owned();
        Ok(CmdOutput { code: status.code().unwrap_or(-1), stdout: out, stderr: err })
    }
    fn spawn_detached(&self, program: &str, args: &[&str]) -> io::Result<()> {
        use std::os::windows::process::CommandExt;
        use std::process::{Command, Stdio};
        Command::new(program).args(args).creation_flags(CREATE_NO_WINDOW)
            .stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null())
            .spawn().map(|_| ())
    }
}

/// Test double: maps (program + first arg) key to canned output.
///
/// **Limitations:** keys are matched on `program + first arg` only; additional
/// arguments are ignored. Output fields are plain `String` values — binary or
/// non-UTF-8 output cannot be represented.
#[derive(Default)]
pub struct MockRunner {
    pub responses: std::collections::HashMap<String, CmdOutput>,
    pub calls: std::sync::Mutex<Vec<String>>,
}
impl MockRunner {
    pub fn on(mut self, key: &str, code: i32, stdout: &str) -> Self {
        self.responses.insert(key.into(), CmdOutput { code, stdout: stdout.into(), stderr: String::new() });
        self
    }
    fn key(program: &str, args: &[&str]) -> String {
        if args.is_empty() { program.to_string() } else { format!("{program} {}", args[0]) }
    }
}
impl Runner for MockRunner {
    fn run(&self, program: &str, args: &[&str], _t: Duration) -> io::Result<CmdOutput> {
        let key = Self::key(program, args);
        self.calls.lock().unwrap().push(format!("{program} {}", args.join(" ")));
        self.responses.get(&key).cloned()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, key))
    }
    fn spawn_detached(&self, program: &str, args: &[&str]) -> io::Result<()> {
        self.calls.lock().unwrap().push(format!("DETACHED {program} {}", args.join(" ")));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mock_runner_returns_configured_output_and_records_calls() {
        let r = MockRunner::default().on("ollama --version", 0, "ollama version is 0.30.6");
        let out = r.run("ollama", &["--version"], Duration::from_secs(5)).unwrap();
        assert!(out.ok());
        assert_eq!(out.stdout, "ollama version is 0.30.6");
        assert!(r.run("missing", &[], Duration::from_secs(1)).is_err());
        assert_eq!(r.calls.lock().unwrap().len(), 2);
    }
    #[test]
    fn system_runner_executes_real_command() {
        let out = SystemRunner.run("cmd", &["/c", "echo hi"], Duration::from_secs(10)).unwrap();
        assert!(out.ok());
        assert!(out.stdout.contains("hi"));
    }
    #[test]
    fn system_runner_times_out_and_kills() {
        let start = std::time::Instant::now();
        let err = SystemRunner.run("powershell", &["-NoProfile", "-Command", "Start-Sleep -Seconds 30"], Duration::from_secs(1)).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::TimedOut);
        assert!(start.elapsed() < Duration::from_secs(10), "should not wait for the child's full 30s");
    }
    #[test]
    fn system_runner_handles_output_larger_than_pipe_buffer() {
        // 1MB of output must come back fully and not be misreported as timeout
        let out = SystemRunner.run("powershell", &["-NoProfile", "-Command", "$s = 'x' * 1048576; Write-Output $s"], Duration::from_secs(60)).unwrap();
        assert!(out.ok());
        assert!(out.stdout.trim_end().len() >= 1_048_576);
    }
}
