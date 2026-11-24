//! Bounded process supervision shared by installation and inference.
#[cfg(unix)]
use std::os::unix::process::CommandExt;
#[cfg(windows)]
use std::os::windows::io::AsRawHandle;
use std::{
    io::{BufRead, BufReader, Read},
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

#[cfg(windows)]
#[allow(
    non_camel_case_types,
    non_snake_case,
    non_upper_case_globals,
    dead_code
)]
pub(crate) mod win32 {
    use std::{ffi::c_void, os::windows::io::RawHandle};

    pub type HANDLE = *mut c_void;
    pub type BOOL = i32;
    pub type DWORD = u32;
    pub type SIZE_T = usize;
    pub type ULONG_PTR = usize;
    pub type LARGE_INTEGER = i64;
    pub type ULONGLONG = u64;

    pub const JobObjectExtendedLimitInformation: u32 = 9;
    pub const JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE: DWORD = 0x00002000;
    pub const PROCESS_QUERY_LIMITED_INFORMATION: DWORD = 0x1000;
    pub const STILL_ACTIVE: DWORD = 259;

    #[repr(C)]
    #[derive(Debug, Default, Copy, Clone)]
    pub struct IO_COUNTERS {
        pub ReadOperationCount: ULONGLONG,
        pub WriteOperationCount: ULONGLONG,
        pub OtherOperationCount: ULONGLONG,
        pub ReadTransferCount: ULONGLONG,
        pub WriteTransferCount: ULONGLONG,
        pub OtherTransferCount: ULONGLONG,
    }

    #[repr(C)]
    #[derive(Debug, Default, Copy, Clone)]
    pub struct JOBOBJECT_BASIC_LIMIT_INFORMATION {
        pub PerProcessUserTimeLimit: LARGE_INTEGER,
        pub PerJobUserTimeLimit: LARGE_INTEGER,
        pub LimitFlags: DWORD,
        pub MinimumWorkingSetSize: SIZE_T,
        pub MaximumWorkingSetSize: SIZE_T,
        pub ActiveProcessLimit: DWORD,
        pub Affinity: ULONG_PTR,
        pub PriorityClass: DWORD,
        pub SchedulingClass: DWORD,
    }

    #[repr(C)]
    #[derive(Debug, Default, Copy, Clone)]
    pub struct JOBOBJECT_EXTENDED_LIMIT_INFORMATION {
        pub BasicLimitInformation: JOBOBJECT_BASIC_LIMIT_INFORMATION,
        pub IoInfo: IO_COUNTERS,
        pub ProcessMemoryLimit: SIZE_T,
        pub JobMemoryLimit: SIZE_T,
        pub PeakProcessMemoryLimit: SIZE_T,
        pub PeakJobMemoryLimit: SIZE_T,
    }

    #[link(name = "kernel32")]
    extern "system" {
        pub fn CreateJobObjectW(lpJobAttributes: *mut c_void, lpName: *const u16) -> HANDLE;
        pub fn SetInformationJobObject(
            hJob: HANDLE,
            JobObjectInformationClass: u32,
            lpJobInformation: *const c_void,
            cbJobInformationLength: DWORD,
        ) -> BOOL;
        pub fn AssignProcessToJobObject(hJob: HANDLE, hProcess: HANDLE) -> BOOL;
        pub fn TerminateJobObject(hJob: HANDLE, uExitCode: u32) -> BOOL;
        pub fn CloseHandle(hObject: HANDLE) -> BOOL;
        pub fn GetLastError() -> DWORD;
        pub fn OpenProcess(
            dwDesiredAccess: DWORD,
            bInheritHandle: BOOL,
            dwProcessId: DWORD,
        ) -> HANDLE;
        pub fn GetExitCodeProcess(hProcess: HANDLE, lpExitCode: *mut DWORD) -> BOOL;
    }

    pub struct JobObject {
        handle: HANDLE,
    }

    unsafe impl Send for JobObject {}
    unsafe impl Sync for JobObject {}

    impl JobObject {
        pub fn create() -> Result<Self, String> {
            let handle = unsafe { CreateJobObjectW(std::ptr::null_mut(), std::ptr::null()) };
            if handle.is_null() {
                let err = unsafe { GetLastError() };
                return Err(format!("Failed to create Windows Job Object: error {err}"));
            }

            let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;

            let set_res = unsafe {
                SetInformationJobObject(
                    handle,
                    JobObjectExtendedLimitInformation,
                    &info as *const _ as *const c_void,
                    std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as DWORD,
                )
            };
            if set_res == 0 {
                let err = unsafe { GetLastError() };
                unsafe { CloseHandle(handle) };
                return Err(format!(
                    "Failed to set JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE: error {err}"
                ));
            }

            Ok(Self { handle })
        }

        pub fn assign_process(&self, process_handle: RawHandle) -> Result<(), String> {
            let res = unsafe { AssignProcessToJobObject(self.handle, process_handle as HANDLE) };
            if res == 0 {
                let err = unsafe { GetLastError() };
                return Err(format!(
                    "Failed to assign process to Windows Job Object: error {err}"
                ));
            }
            Ok(())
        }

        pub fn terminate(&self, exit_code: u32) -> Result<(), String> {
            if !self.handle.is_null() {
                let res = unsafe { TerminateJobObject(self.handle, exit_code) };
                if res == 0 {
                    let err = unsafe { GetLastError() };
                    return Err(format!("Failed to terminate Job Object: error {err}"));
                }
            }
            Ok(())
        }
    }

    impl Drop for JobObject {
        fn drop(&mut self) {
            if !self.handle.is_null() {
                unsafe {
                    let _ = TerminateJobObject(self.handle, 1);
                    CloseHandle(self.handle);
                }
                self.handle = std::ptr::null_mut();
            }
        }
    }
}

struct OwnedChild {
    child: Child,
    kill_on_drop: bool,
    #[cfg(windows)]
    job: Option<win32::JobObject>,
}

impl Drop for OwnedChild {
    fn drop(&mut self) {
        if self.kill_on_drop {
            // Terminate the group, then kill after a short grace period. On macOS,
            // a concurrent posix_spawn can finish creating a descendant just after
            // the first signal. Keep the leader unreaped so its group ID is reserved.
            #[cfg(unix)]
            unsafe {
                let group = -(self.child.id() as i32);
                libc::kill(group, libc::SIGTERM);
                std::thread::sleep(Duration::from_millis(100));
                libc::kill(group, libc::SIGKILL);
            }
            #[cfg(windows)]
            {
                if let Some(ref job) = self.job {
                    let _ = job.terminate(1);
                }
            }
            let _ = self.child.kill();
            let _ = self.child.wait();
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

    #[cfg(windows)]
    let job = win32::JobObject::create()
        .map_err(|e| format!("Could not initialize process supervisor: {e}"))?;

    let mut spawned = command
        .spawn()
        .map_err(|e| format!("Could not start local process: {e}"))?;

    #[cfg(windows)]
    {
        let raw_handle = spawned.as_raw_handle();
        if let Err(e) = job.assign_process(raw_handle) {
            if spawned.try_wait().map(|s| s.is_none()).unwrap_or(true) {
                let _ = spawned.kill();
                let _ = spawned.wait();
                return Err(format!("Could not supervise local process tree: {e}"));
            }
        }
    }

    let mut child = OwnedChild {
        child: spawned,
        kill_on_drop: true,
        #[cfg(windows)]
        job: Some(job),
    };

    let stdout = child
        .child
        .stdout
        .take()
        .ok_or("Process output unavailable")?;
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
            if let Some(status) = child.child.try_wait().map_err(|e| e.to_string())? {
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
        child.kill_on_drop = false;
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
        #[cfg(unix)]
        let mut cmd = {
            let mut c = Command::new("/bin/sh");
            c.args(["-c", "exec 1>&-; sleep 10"]);
            c
        };
        #[cfg(windows)]
        let mut cmd = {
            let mut c = Command::new("python");
            c.args(["-c", "import os, time; os.close(1); time.sleep(10)"]);
            c
        };
        let error = run(
            &mut cmd,
            &AtomicBool::new(false),
            Duration::from_millis(150),
            |_| Ok(()),
        )
        .unwrap_err();
        assert!(error.contains("timed out"), "{error}");
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn cancellation_stops_process_group_and_releases_reader() {
        let started = Instant::now();
        let cancelled = AtomicBool::new(false);
        #[cfg(unix)]
        let mut cmd = {
            let mut c = Command::new("/bin/sh");
            c.args(["-c", "echo started; sleep 10 & wait"]);
            c
        };
        #[cfg(windows)]
        let mut cmd = {
            let mut c = Command::new("cmd");
            c.args(["/c", "echo started & ping 127.0.0.1 -n 11 >nul"]);
            c
        };
        let error = run(&mut cmd, &cancelled, Duration::from_secs(5), |_| {
            cancelled.store(true, Ordering::Relaxed);
            Ok(())
        })
        .unwrap_err();
        assert!(error.contains("cancelled"), "{error}");
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn reads_output_and_detects_nonzero_exit() {
        let mut lines = Vec::new();
        #[cfg(unix)]
        let mut cmd = {
            let mut c = Command::new("/bin/sh");
            c.args(["-c", "echo output; exit 3"]);
            c
        };
        #[cfg(windows)]
        let mut cmd = {
            let mut c = Command::new("cmd");
            c.args(["/c", "echo output & exit /b 3"]);
            c
        };
        assert!(run(
            &mut cmd,
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
        #[cfg(unix)]
        let mut cmd = {
            let mut c = Command::new("/bin/sh");
            c.args(["-c", "echo $SCULPT_PARENT_PID"]);
            c
        };
        #[cfg(windows)]
        let mut cmd = {
            let mut c = Command::new("cmd");
            c.args(["/c", "echo %SCULPT_PARENT_PID%"]);
            c
        };
        run(
            &mut cmd,
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
        #[cfg(unix)]
        let mut cmd = {
            let mut c = Command::new("/bin/sh");
            c.args(["-c", "while :; do echo progress; done"]);
            c
        };
        #[cfg(windows)]
        let mut cmd = {
            let mut c = Command::new("cmd");
            c.args(["/c", "for /l %i in (1,0,2) do @echo progress"]);
            c
        };
        let error = run(&mut cmd, &cancelled, Duration::from_secs(5), |_| {
            // Allow the worker to fill the bounded queue before cancellation.
            std::thread::sleep(Duration::from_millis(100));
            cancelled.store(true, Ordering::Relaxed);
            Ok(())
        })
        .unwrap_err();
        assert!(error.contains("cancelled"), "{error}");
        assert!(started.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn malformed_output_terminates_the_worker() {
        #[cfg(unix)]
        let cases = [
            (
                {
                    let mut c = Command::new("/bin/sh");
                    c.args(["-c", "printf '\\377\\n'; sleep 10"]);
                    c
                },
                "invalid text",
            ),
            (
                {
                    let mut c = Command::new("/bin/sh");
                    c.args(["-c", "head -c 65537 /dev/zero; sleep 10"]);
                    c
                },
                "oversized message",
            ),
        ];

        #[cfg(windows)]
        let cases = [
            (
                {
                    let mut c = Command::new("python");
                    c.args([
                        "-c",
                        "import sys, time; sys.stdout.buffer.write(b'\\xff\\n'); sys.stdout.buffer.flush(); time.sleep(10)",
                    ]);
                    c
                },
                "invalid text",
            ),
            (
                {
                    let mut c = Command::new("python");
                    c.args([
                        "-c",
                        "import sys, time; sys.stdout.buffer.write(b'A'*65537); sys.stdout.buffer.flush(); time.sleep(10)",
                    ]);
                    c
                },
                "oversized message",
            ),
        ];

        for (mut cmd, expected) in cases {
            let started = Instant::now();
            let error = run(
                &mut cmd,
                &AtomicBool::new(false),
                Duration::from_secs(5),
                |_| panic!("Malformed output reached the event parser"),
            )
            .unwrap_err();
            assert!(error.contains(expected), "{error}");
            assert!(started.elapsed() < Duration::from_secs(2));
        }
    }

    #[cfg(windows)]
    #[test]
    fn windows_job_object_terminates_descendant_tree() {
        use std::sync::atomic::AtomicU32;
        use std::sync::Arc;

        let child_pid = Arc::new(AtomicU32::new(0));
        let child_pid_clone = Arc::clone(&child_pid);
        let cancelled = Arc::new(AtomicBool::new(false));
        let cancelled_clone = Arc::clone(&cancelled);
        let started = Instant::now();

        // Launch a python script that spawns a child process sleeping 15s and prints its PID
        let script = r#"
import subprocess, sys, time
proc = subprocess.Popen([sys.executable, "-c", "import time; time.sleep(15)"])
print(f"CHILD_PID:{proc.pid}", flush=True)
time.sleep(15)
"#;
        let mut cmd = Command::new("python");
        cmd.args(["-c", script]);

        let error = run(&mut cmd, &cancelled, Duration::from_secs(5), move |line| {
            if let Some(pid_str) = line.strip_prefix("CHILD_PID:") {
                if let Ok(pid) = pid_str.trim().parse::<u32>() {
                    child_pid_clone.store(pid, Ordering::Relaxed);
                    cancelled_clone.store(true, Ordering::Relaxed);
                }
            }
            Ok(())
        })
        .unwrap_err();

        assert!(error.contains("cancelled"), "{error}");
        assert!(started.elapsed() < Duration::from_secs(3));

        let pid = child_pid.load(Ordering::Relaxed);
        assert_ne!(pid, 0, "Child PID should have been reported");

        // Give the Windows kernel a moment to complete job termination
        std::thread::sleep(Duration::from_millis(150));

        // Verify that the descendant child process was terminated by the Job Object
        let handle =
            unsafe { win32::OpenProcess(win32::PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
        if !handle.is_null() {
            let mut exit_code: win32::DWORD = 0;
            let ok = unsafe { win32::GetExitCodeProcess(handle, &mut exit_code) };
            unsafe { win32::CloseHandle(handle) };
            if ok != 0 {
                assert_ne!(
                    exit_code,
                    win32::STILL_ACTIVE,
                    "Descendant process {pid} was not terminated by Job Object!"
                );
            }
        }
    }
}
