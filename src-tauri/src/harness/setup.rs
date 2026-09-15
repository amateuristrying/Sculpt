use super::{paths, process, python, runtime::JobContext, SculptInferenceHarness};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    path::Path,
    process::{Command, Stdio},
    sync::{atomic::AtomicBool, Arc},
    time::Duration,
};
use tauri::{AppHandle, Emitter, State};

const UV_URL: &str =
    "https://github.com/astral-sh/uv/releases/download/0.10.10/uv-aarch64-apple-darwin.tar.gz";
const UV_HASH: &str = "8a09f0ef51ee7f7170731b4cb8bde5bf9ba6da5304f49a7df6cdab42a1f37b5d";

pub fn install(root: &Path, script: &Path, context: &JobContext) -> Result<(), String> {
    std::fs::create_dir_all(root).map_err(|e| e.to_string())?;
    let log_path = root.join("setup.log");
    let log = std::fs::File::create(&log_path).map_err(|e| e.to_string())?;
    let tools = root.join("tools");
    std::fs::create_dir_all(&tools).map_err(|e| e.to_string())?;
    let uv = tools.join("uv");
    if !uv.is_file() {
        context.emit(
            "bootstrap",
            3.0,
            "Downloading the isolated runtime manager · about 20 MB",
        );
        let archive = tools.join("uv.tar.gz");
        process::run(
            Command::new("/usr/bin/curl")
                .args([
                    "--fail",
                    "--silent",
                    "--show-error",
                    "--location",
                    "--proto",
                    "=https",
                    "--proto-redir",
                    "=https",
                    "--max-time",
                    "300",
                    "--max-filesize",
                    "40000000",
                    "--retry",
                    "2",
                    "-o",
                ])
                .arg(&archive)
                .arg(UV_URL)
                .stderr(Stdio::from(log.try_clone().map_err(|e| e.to_string())?)),
            &context.cancelled,
            Duration::from_secs(360),
            |_| Ok(()),
        )?;
        let digest = format!(
            "{:x}",
            Sha256::digest(std::fs::read(&archive).map_err(|e| e.to_string())?)
        );
        if digest != UV_HASH {
            return Err("Runtime manager checksum failed. Retry setup.".into());
        }
        process::run(
            Command::new("/usr/bin/tar")
                .arg("-xzf")
                .arg(&archive)
                .arg("-C")
                .arg(&tools)
                .args(["--strip-components", "1", "uv-aarch64-apple-darwin/uv"])
                .stderr(Stdio::from(log.try_clone().map_err(|e| e.to_string())?)),
            &context.cancelled,
            Duration::from_secs(30),
            |_| Ok(()),
        )?;
    }
    context.emit(
        "python",
        10.0,
        "Preparing Python for Sculpt · first setup requires internet",
    );
    let mut command = Command::new(&uv);
    command
        .args([
            "run",
            "--no-project",
            "--no-config",
            "--python",
            "3.11",
            "--",
        ])
        .arg(script)
        .env("SCULPT_RUNTIME_DIR", root)
        .env("SCULPT_UV", &uv)
        .env("SCULPT_PARENT_PID", std::process::id().to_string())
        .env("UV_PYTHON_INSTALL_DIR", root.join("python"))
        .env("UV_CACHE_DIR", root.join("package-cache"))
        .env("UV_NO_CONFIG", "1")
        .env("UV_NO_PROGRESS", "1")
        .env("PYTHONUNBUFFERED", "1")
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .stderr(Stdio::from(log));
    let mut complete = false;
    let mut last = 10.0;
    process::run(
        &mut command,
        &context.cancelled,
        Duration::from_secs(3600),
        |line| {
            let event: Value =
                serde_json::from_str(line).map_err(|_| "Installer returned an invalid response")?;
            if event["protocol"] != 1 {
                return Err("Unsupported installer protocol".into());
            }
            match event["type"].as_str() {
                Some("progress") => {
                    last = event["progress"].as_f64().unwrap_or(last).clamp(last, 99.0);
                    context.emit(
                        event["stage"].as_str().unwrap_or("setup"),
                        last,
                        event["message"]
                            .as_str()
                            .unwrap_or("Preparing local runtime"),
                    );
                }
                Some("result") => complete = event["installed"].as_bool() == Some(true),
                Some("error") => {
                    return Err(event["message"]
                        .as_str()
                        .unwrap_or("Runtime setup failed")
                        .to_owned())
                }
                _ => return Err("Unexpected installer message".into()),
            }
            Ok(())
        },
    )
    .map_err(|e| format!("{e} Details: {}", log_path.display()))?;
    if !complete || !python::inspect(root).installed {
        return Err("Runtime setup did not pass verification. Retry repair.".into());
    }
    if !python::inspect(root).mps_available {
        return Err("Model files are installed, but PyTorch could not access Metal. Restart Sculpt and verify the runtime.".into());
    }
    context.emit("complete", 100.0, "Your local engine is ready");
    Ok(())
}

#[tauri::command]
pub async fn install_runtime(
    app: AppHandle,
    harness: State<'_, SculptInferenceHarness>,
    job_id: String,
) -> Result<python::BackendStatus, String> {
    uuid::Uuid::parse_str(&job_id).map_err(|_| "Invalid setup identifier")?;
    let hardware = tauri::async_runtime::spawn_blocking(super::hardware::detect)
        .await
        .map_err(|e| e.to_string())?;
    if !hardware.is_apple_silicon || hardware.memory_gb < 16.0 {
        return Err(
            "This runtime currently requires an Apple Silicon Mac with 16 GB memory.".into(),
        );
    }
    let root = paths::runtime_dir(&app)?;
    let script = paths::backend_file(&app, "setup_runtime.py")?;
    let cancelled = Arc::new(AtomicBool::new(false));
    {
        let mut jobs = harness.jobs.lock().map_err(|_| "Job state unavailable")?;
        if !jobs.is_empty() {
            return Err("Finish or cancel the active operation before setup.".into());
        }
        jobs.insert(job_id.clone(), cancelled.clone());
    }
    let cloned_root = root.clone();
    let context = JobContext {
        id: job_id.clone(),
        cancelled,
        progress: Arc::new(move |event| {
            let _ = app.emit_to("main", "sculpt://setup-progress", event);
        }),
    };
    let result =
        tauri::async_runtime::spawn_blocking(move || install(&cloned_root, &script, &context))
            .await
            .map_err(|e| e.to_string())
            .and_then(|r| r);
    if let Ok(mut jobs) = harness.jobs.lock() {
        jobs.remove(&job_id);
    }
    result?;
    Ok(python::inspect(&root))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "Downloads a local runtime; set SCULPT_TEST_SETUP_ROOT to an isolated folder"]
    fn installs_runtime_through_native_supervisor() {
        let root = std::path::PathBuf::from(
            std::env::var_os("SCULPT_TEST_SETUP_ROOT").expect("SCULPT_TEST_SETUP_ROOT"),
        );
        let script = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("backend/setup_runtime.py");
        let context = JobContext {
            id: uuid::Uuid::new_v4().to_string(),
            cancelled: Arc::new(AtomicBool::new(false)),
            progress: Arc::new(|p| println!("{} {}", p.progress, p.message)),
        };
        install(&root, &script, &context).unwrap();
        assert!(python::inspect(&root).installed);
    }
}
