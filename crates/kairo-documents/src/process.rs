//! Child processes with a time limit, as `execFile`'s `timeout` gives them.

use std::io::Read;
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

/// Runs `command` to completion, killing it after `timeout`. Output beyond
/// 1 MB is dropped, like `maxBuffer` (which fails instead).
pub fn run(command: &mut Command, timeout: Duration) -> std::io::Result<Output> {
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let drain = |pipe: Option<Box<dyn Read + Send>>| {
        std::thread::spawn(move || {
            let mut out = Vec::new();
            if let Some(pipe) = pipe {
                let _ = pipe.take(1024 * 1024).read_to_end(&mut out);
            }
            out
        })
    };
    let out = drain(child.stdout.take().map(|p| Box::new(p) as _));
    let err = drain(child.stderr.take().map(|p| Box::new(p) as _));
    let deadline = Instant::now() + timeout;
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "Command timed out.",
            ));
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    Ok(Output {
        status,
        stdout: out.join().unwrap_or_default(),
        stderr: err.join().unwrap_or_default(),
    })
}

/// Runs `command`, failing on a non-zero exit with its stderr.
pub fn run_ok(command: &mut Command, timeout: Duration) -> std::io::Result<Output> {
    let output = run(command, timeout)?;
    if output.status.success() {
        return Ok(output);
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    let message = kairo_domain::js::trim(&stderr);
    Err(std::io::Error::other(if message.is_empty() {
        format!("Command failed: {}", output.status)
    } else {
        message.to_owned()
    }))
}
