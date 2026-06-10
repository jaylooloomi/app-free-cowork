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

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

impl Runner for SystemRunner {
    fn run(&self, program: &str, args: &[&str], timeout: Duration) -> io::Result<CmdOutput> {
        use std::os::windows::process::CommandExt;
        use std::process::{Command, Stdio};
        use wait_timeout::ChildExt;
        let mut child = Command::new(program)
            .args(args)
            .creation_flags(CREATE_NO_WINDOW)
            .stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped())
            .spawn()?;
        let status = match child.wait_timeout(timeout)? {
            Some(s) => s,
            None => { let _ = child.kill(); return Err(io::Error::new(io::ErrorKind::TimedOut, format!("{program} timed out"))); }
        };
        let mut out = String::new(); let mut err = String::new();
        use std::io::Read;
        if let Some(mut s) = child.stdout.take() { let _ = s.read_to_string(&mut out); }
        if let Some(mut s) = child.stderr.take() { let _ = s.read_to_string(&mut err); }
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
}
