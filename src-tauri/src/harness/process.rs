//! Bounded process supervision shared by installation and inference.
#[cfg(unix)]
use std::os::unix::process::CommandExt;
use std::{
    io::{BufRead, BufReader, Read},
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

struct OwnedChild(Child, bool);
impl Drop for OwnedChild {
    fn drop(&mut self) {
        if self.1 {
            // Terminate the group, then kill after a short grace period. On macOS,
            // a concurrent posix_spawn can finish creating a descendant just after
            // the first signal. Keep the leader unreaped so its group ID is reserved.
            #[cfg(unix)]
            unsafe {
                let group = -(self.0.id() as i32);
                libc::kill(group, libc::SIGTERM);
                std::thread::sleep(Duration::from_millis(100));
                libc::kill(group, libc::SIGKILL);
            }
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}

pub fn run(
    command: &mut Command,
    cancelled: &AtomicBool,
    timeout: Duration,
    mut on_line: impl FnMut(&str) -> Result<(), String>,
) -> Result<(), String> {
    if cancelled.load(Ordering::Relaxed) {
        return Err("Operation cancelled".into());
    }
    #[cfg(unix)]
    command.process_group(0);
    command
        .env("SCULPT_PARENT_PID", std::process::id().to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::piped());
    let mut child = OwnedChild(
        command
            .spawn()
            .map_err(|e| format!("Could not start local process: {e}"))?,
        true,
    );
    let stdout = child.0.stdout.take().ok_or("Process output unavailable")?;
    // Bound queued output as well as each line. A noisy or broken worker must
    // not consume unbounded host memory while the UI handles progress events.
    let (tx, rx) = mpsc::sync_channel(16);
    let reader = std::thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        loop {
            let mut line = Vec::new();
            let result = reader.by_ref().take(65537).read_until(b'\n', &mut line);
            if matches!(result, Ok(0)) {
                break;
            }
            let message = result.map_err(|e| e.to_string()).and_then(|_| {
                if line.len() > 65536 {
                    Err("Local process emitted an oversized message".into())
                } else {
                    String::from_utf8(line).map_err(|_| "Local process emitted invalid text".into())
                }
            });
            let failed = message.is_err();
            if tx.send(message).is_err() || failed {
                break;
            }
        }
    });
    let start = Instant::now();
    let mut closed = false;
    let outcome = (|| loop {
        if cancelled.load(Ordering::Relaxed) {
            return Err("Operation cancelled".into());
        }
        if start.elapsed() > timeout {
            return Err(
                "Local operation timed out. Try again, or use a lower geometry quality.".into(),
            );
        }
        if closed {
            if let Some(status) = child.0.try_wait().map_err(|e| e.to_string())? {
                return if status.success() {
                    Ok(())
                } else {
                    Err(format!(
                        "Local process stopped ({status}). Check the local operation log."
                    ))
                };
            }
            std::thread::sleep(Duration::from_millis(50));
        } else {
            match rx.recv_timeout(Duration::from_millis(100)) {
                Ok(Ok(line)) => on_line(line.trim_end())?,
                Ok(Err(error)) => return Err(error),
                Err(mpsc::RecvTimeoutError::Disconnected) => closed = true,
                Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
        }
    })();
    if outcome.is_ok() {
        child.1 = false;
    }
    drop(child); // Release GPU and descendants before reporting cancellation.
    drop(rx);
    let _ = reader.join();
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn closed_stdout_cannot_bypass_timeout() {
        let started = Instant::now();
        let error = run(
            Command::new("/bin/sh").args(["-c", "exec 1>&-; sleep 10"]),
            &AtomicBool::new(false),
            Duration::from_millis(150),
            |_| Ok(()),
        )
        .unwrap_err();
        assert!(error.contains("timed out"));
        assert!(started.elapsed() < Duration::from_secs(2));
    }
    #[test]
    fn cancellation_stops_process_group_and_releases_reader() {
        let started = Instant::now();
        let cancelled = AtomicBool::new(false);
        let error = run(
            Command::new("/bin/sh").args(["-c", "echo started; sleep 10 & wait"]),
            &cancelled,
            Duration::from_secs(5),
            |_| {
                cancelled.store(true, Ordering::Relaxed);
                Ok(())
            },
        )
        .unwrap_err();
        assert!(error.contains("cancelled"));
        assert!(started.elapsed() < Duration::from_secs(2));
    }
    #[test]
    fn reads_output_and_detects_nonzero_exit() {
        let mut lines = Vec::new();
        assert!(run(
            Command::new("/bin/sh").args(["-c", "echo output; exit 3"]),
            &AtomicBool::new(false),
            Duration::from_secs(2),
            |line| {
                lines.push(line.to_owned());
                Ok(())
            }
        )
        .is_err());
        assert_eq!(lines, ["output"]);
    }

    #[test]
    fn worker_knows_its_parent_before_startup() {
        let mut parent = String::new();
        run(
            Command::new("/bin/sh").args(["-c", "echo $SCULPT_PARENT_PID"]),
            &AtomicBool::new(false),
            Duration::from_secs(2),
            |line| {
                parent = line.to_owned();
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(parent, std::process::id().to_string());
    }

    #[test]
    fn cancelling_a_flooding_worker_does_not_block_on_its_output_queue() {
        let cancelled = AtomicBool::new(false);
        let started = Instant::now();
        let error = run(
            Command::new("/bin/sh").args(["-c", "while :; do echo progress; done"]),
            &cancelled,
            Duration::from_secs(5),
            |_| {
                // Allow the worker to fill the bounded queue before cancellation.
                std::thread::sleep(Duration::from_millis(100));
                cancelled.store(true, Ordering::Relaxed);
                Ok(())
            },
        )
        .unwrap_err();
        assert!(error.contains("cancelled"));
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn malformed_output_terminates_the_worker() {
        for (script, expected) in [
            ("printf '\\377\\n'; sleep 10", "invalid text"),
            ("head -c 65537 /dev/zero; sleep 10", "oversized message"),
        ] {
            let started = Instant::now();
            let error = run(
                Command::new("/bin/sh").args(["-c", script]),
                &AtomicBool::new(false),
                Duration::from_secs(5),
                |_| panic!("Malformed output reached the event parser"),
            )
            .unwrap_err();
            assert!(error.contains(expected), "{error}");
            assert!(started.elapsed() < Duration::from_secs(2));
        }
    }
}
