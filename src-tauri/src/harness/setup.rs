use super::{paths, process, python, runtime::JobContext, SculptInferenceHarness};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{atomic::AtomicBool, Arc},
    time::Duration,
};
use tauri::{AppHandle, Emitter, State};

/// Supported platform-specific uv binary release distribution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UvRelease {
    pub url: &'static str,
    pub sha256: &'static str,
    pub archive_filename: &'static str,
    pub binary_name: &'static str,
    pub is_zip: bool,
    pub archive_member_prefix: Option<&'static str>,
}

/// Resolves the official pinned uv distribution for the given OS and architecture.
pub fn uv_release_for(os: &str, arch: &str) -> Result<UvRelease, String> {
    match (os, arch) {
        ("macos", "aarch64") | ("darwin", "aarch64") | ("macos", "arm64") | ("darwin", "arm64") => {
            Ok(UvRelease {
                url: "https://github.com/astral-sh/uv/releases/download/0.10.10/uv-aarch64-apple-darwin.tar.gz",
                sha256: "8a09f0ef51ee7f7170731b4cb8bde5bf9ba6da5304f49a7df6cdab42a1f37b5d",
                archive_filename: "uv-aarch64-apple-darwin.tar.gz",
                binary_name: "uv",
                is_zip: false,
                archive_member_prefix: Some("uv-aarch64-apple-darwin"),
            })
        }
        ("macos", "x86_64") | ("darwin", "x86_64") => {
            Ok(UvRelease {
                url: "https://github.com/astral-sh/uv/releases/download/0.10.10/uv-x86_64-apple-darwin.tar.gz",
                sha256: "dd18420591d625f9b4ca2b57a7a6fe3cce43910f02e02d90e47a4101428de14a",
                archive_filename: "uv-x86_64-apple-darwin.tar.gz",
                binary_name: "uv",
                is_zip: false,
                archive_member_prefix: Some("uv-x86_64-apple-darwin"),
            })
        }
        ("linux", "x86_64") => {
            Ok(UvRelease {
                url: "https://github.com/astral-sh/uv/releases/download/0.10.10/uv-x86_64-unknown-linux-gnu.tar.gz",
                sha256: "3e1027f26ce8c7e4c32e2277a7fed2cb410f2f1f9320d3df97653d40e21f415b",
                archive_filename: "uv-x86_64-unknown-linux-gnu.tar.gz",
                binary_name: "uv",
                is_zip: false,
                archive_member_prefix: Some("uv-x86_64-unknown-linux-gnu"),
            })
        }
        ("linux", "aarch64") | ("linux", "arm64") => {
            Ok(UvRelease {
                url: "https://github.com/astral-sh/uv/releases/download/0.10.10/uv-aarch64-unknown-linux-gnu.tar.gz",
                sha256: "2b80457b950deda12e8d5dc3b9b7494ac143eae47f1fb11b1c6e5a8495a6421e",
                archive_filename: "uv-aarch64-unknown-linux-gnu.tar.gz",
                binary_name: "uv",
                is_zip: false,
                archive_member_prefix: Some("uv-aarch64-unknown-linux-gnu"),
            })
        }
        ("windows", "x86_64") => {
            Ok(UvRelease {
                url: "https://github.com/astral-sh/uv/releases/download/0.10.10/uv-x86_64-pc-windows-msvc.zip",
                sha256: "d31a30f1dfb96e630a08d5a9b3f3f551254b7ed6e9b7e495f46a4232661c7252",
                archive_filename: "uv-x86_64-pc-windows-msvc.zip",
                binary_name: "uv.exe",
                is_zip: true,
                archive_member_prefix: None,
            })
        }
        ("windows", "aarch64") | ("windows", "arm64") => {
            Ok(UvRelease {
                url: "https://github.com/astral-sh/uv/releases/download/0.10.10/uv-aarch64-pc-windows-msvc.zip",
                sha256: "2c6fe113f14574bc27f085751c68d3485589fcc3c3c64ed85dd1eecc2f87cffc",
                archive_filename: "uv-aarch64-pc-windows-msvc.zip",
                binary_name: "uv.exe",
                is_zip: true,
                archive_member_prefix: None,
            })
        }
        _ => Err(format!(
            "Unsupported platform for uv runtime manager: os='{}', arch='{}'",
            os, arch
        )),
    }
}

/// Resolves the uv distribution for the current host operating system and architecture.
pub fn current_uv_release() -> Result<UvRelease, String> {
    uv_release_for(std::env::consts::OS, std::env::consts::ARCH)
}

/// Portably finds an executable on the system, checking standard system directories and PATH.
pub fn find_executable(name: &str) -> Option<PathBuf> {
    let path = Path::new(name);
    if path.is_file() {
        return Some(path.to_path_buf());
    }

    #[cfg(windows)]
    let candidates = {
        let mut list = Vec::new();
        if let Ok(sysroot) = std::env::var("SystemRoot") {
            let sys32 = Path::new(&sysroot).join("System32");
            list.push(sys32.join(format!("{}.exe", name)));
            list.push(sys32.join(name));
        }
        list
    };

    #[cfg(not(windows))]
    let candidates = {
        let mut list = Vec::new();
        list.push(PathBuf::from(format!("/usr/bin/{}", name)));
        list.push(PathBuf::from(format!("/bin/{}", name)));
        list.push(PathBuf::from(format!("/usr/local/bin/{}", name)));
        list
    };

    for candidate in candidates {
        if candidate.is_file() {
            return Some(candidate);
        }
    }

    if let Some(paths) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&paths) {
            #[cfg(windows)]
            {
                let exe = dir.join(format!("{}.exe", name));
                if exe.is_file() {
                    return Some(exe);
                }
            }
            let direct = dir.join(name);
            if direct.is_file() {
                return Some(direct);
            }
        }
    }
    None
}

/// Resolves the uv executable to use:
/// 1. `SCULPT_UV` environment variable if explicitly specified and present.
/// 2. Locally downloaded/extracted uv in `<tools_dir>`.
/// 3. System-installed uv on PATH (e.g. on developer workstations).
pub fn resolve_uv(tools_dir: &Path) -> Option<PathBuf> {
    if let Some(custom) = std::env::var_os("SCULPT_UV") {
        let p = PathBuf::from(custom);
        if p.is_file() {
            return Some(p);
        }
    }
    let local_uv = if cfg!(windows) {
        tools_dir.join("uv.exe")
    } else {
        tools_dir.join("uv")
    };
    if local_uv.is_file() {
        return Some(local_uv);
    }
    if let Some(system_uv) = find_executable("uv") {
        return Some(system_uv);
    }
    None
}

/// Validates archive entry paths to prevent directory traversal or absolute-path overwrites.
pub fn validate_archive_entries<S: AsRef<str>>(entries: &[S]) -> Result<(), String> {
    for entry in entries {
        let entry_str = entry.as_ref().trim();
        if entry_str.is_empty() {
            continue;
        }
        let path = Path::new(entry_str);
        if path.is_absolute() || entry_str.starts_with('/') || entry_str.starts_with('\\') {
            return Err(format!("Unsafe archive path (absolute path): {entry_str}"));
        }
        if entry_str.len() >= 2 && entry_str.as_bytes()[1] == b':' {
            return Err(format!("Unsafe archive path (drive prefix): {entry_str}"));
        }
        for component in path.components() {
            match component {
                std::path::Component::ParentDir => {
                    return Err(format!(
                        "Unsafe archive path (parent directory traversal): {entry_str}"
                    ));
                }
                std::path::Component::RootDir | std::path::Component::Prefix(_) => {
                    return Err(format!(
                        "Unsafe archive path (root/prefix component): {entry_str}"
                    ));
                }
                std::path::Component::CurDir | std::path::Component::Normal(_) => {}
            }
        }
    }
    Ok(())
}

/// Downloads, validates, and unpacks the uv binary into `tools` if not already available.
pub fn ensure_uv(
    tools: &Path,
    log: &std::fs::File,
    context: &JobContext,
) -> Result<PathBuf, String> {
    if let Some(uv) = resolve_uv(tools) {
        return Ok(uv);
    }

    let release = current_uv_release()?;
    context.emit(
        "bootstrap",
        3.0,
        "Downloading the isolated runtime manager · about 20 MB",
    );

    let curl_cmd = find_executable("curl").ok_or_else(|| {
        "Required 'curl' command not found on system. Please ensure curl is installed and on PATH."
            .to_string()
    })?;
    let tar_cmd = find_executable("tar").ok_or_else(|| {
        "Required 'tar' command not found on system. Please ensure tar is installed and on PATH."
            .to_string()
    })?;

    let archive = tools.join(release.archive_filename);
    let mut curl = Command::new(&curl_cmd);
    curl.args([
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
        "80000000",
        "--retry",
        "2",
        "-C",
        "-",
        "-o",
    ])
    .arg(&archive)
    .arg(release.url)
    .stderr(Stdio::from(log.try_clone().map_err(|e| e.to_string())?));

    process::run(
        &mut curl,
        &context.cancelled,
        Duration::from_secs(360),
        |_| Ok(()),
    )?;

    let archive_bytes = std::fs::read(&archive).map_err(|e| e.to_string())?;
    let digest = format!("{:x}", Sha256::digest(&archive_bytes));
    if digest != release.sha256 {
        let _ = std::fs::remove_file(&archive);
        return Err("Runtime manager checksum failed. Retry setup.".into());
    }

    // Inspect archive contents to prevent path traversal before extraction
    let mut list_cmd = Command::new(&tar_cmd);
    if release.is_zip {
        list_cmd.arg("-tf").arg(&archive);
    } else {
        list_cmd.arg("-tzf").arg(&archive);
    }
    let mut entries = Vec::new();
    process::run(
        &mut list_cmd,
        &context.cancelled,
        Duration::from_secs(30),
        |line| {
            entries.push(line.trim().to_string());
            Ok(())
        },
    )?;
    validate_archive_entries(&entries)?;

    // Extract binary
    let mut extract_cmd = Command::new(&tar_cmd);
    if release.is_zip {
        extract_cmd
            .arg("-xf")
            .arg(&archive)
            .arg("-C")
            .arg(tools)
            .arg(release.binary_name);
    } else {
        extract_cmd.arg("-xzf").arg(&archive).arg("-C").arg(tools);
        if let Some(prefix) = release.archive_member_prefix {
            extract_cmd.args([
                "--strip-components",
                "1",
                &format!("{prefix}/{}", release.binary_name),
            ]);
        } else {
            extract_cmd.arg(release.binary_name);
        }
    }
    extract_cmd.stderr(Stdio::from(log.try_clone().map_err(|e| e.to_string())?));
    process::run(
        &mut extract_cmd,
        &context.cancelled,
        Duration::from_secs(30),
        |_| Ok(()),
    )?;

    let uv_bin = tools.join(release.binary_name);
    if !uv_bin.is_file() {
        return Err("Runtime manager extraction failed: binary not found".into());
    }
    Ok(uv_bin)
}

/// Evaluates hardware resource preflight for runtime installation across platforms.
pub fn validate_setup_hardware(hardware: &super::hardware::HardwareProfile) -> Result<(), String> {
    if (hardware.os.eq_ignore_ascii_case("macos") || hardware.os.eq_ignore_ascii_case("darwin"))
        && !hardware.is_apple_silicon
    {
        return Err("On macOS, Sculpt requires an Apple Silicon Mac (M1 or later).".into());
    }
    if hardware.memory_gb.is_finite() && hardware.memory_gb > 0.0 && hardware.memory_gb < 8.0 {
        return Err(format!(
            "Runtime setup requires at least 8 GB of physical memory (detected {:.1} GB).",
            hardware.memory_gb
        ));
    }
    if let Some(storage) = hardware.storage_gb {
        if storage < 5.0 {
            return Err(format!(
                "At least 5 GB of free disk space is required for runtime setup (detected {:.1} GB free).",
                storage
            ));
        }
    }
    Ok(())
}

/// Cleans or uninstalls runtime components.
/// If `keep_models` is true, downloaded models and package caches are retained for rapid repair.
pub fn uninstall_runtime_files(root: &Path, keep_models: bool) -> Result<(), String> {
    let remove_if_exists = |path: PathBuf| -> Result<(), String> {
        if path.is_file() {
            std::fs::remove_file(&path)
                .map_err(|e| format!("Failed to remove {}: {e}", path.display()))?;
        } else if path.is_dir() {
            std::fs::remove_dir_all(&path)
                .map_err(|e| format!("Failed to remove {}: {e}", path.display()))?;
        }
        Ok(())
    };

    remove_if_exists(root.join("ready.json"))?;
    remove_if_exists(root.join("ready.json.tmp"))?;
    remove_if_exists(root.join("venv"))?;
    remove_if_exists(root.join("python"))?;
    remove_if_exists(root.join("TripoSR"))?;
    remove_if_exists(root.join("setup.log"))?;
    remove_if_exists(root.join("source.tar.gz"))?;

    if !keep_models {
        remove_if_exists(root.join("models"))?;
        remove_if_exists(root.join("tools"))?;
        let cache = paths::cache_dir(root);
        remove_if_exists(cache)?;
    }

    Ok(())
}

pub fn install(root: &Path, script: &Path, context: &JobContext) -> Result<(), String> {
    std::fs::create_dir_all(root).map_err(|e| e.to_string())?;
    let log_path = root.join("setup.log");
    let log = std::fs::File::create(&log_path).map_err(|e| e.to_string())?;
    let tools = root.join("tools");
    std::fs::create_dir_all(&tools).map_err(|e| e.to_string())?;

    let uv = ensure_uv(&tools, &log, context)?;

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
        .env("UV_CACHE_DIR", paths::cache_dir(root))
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

    let status = python::inspect(root);
    if !complete || !status.installed {
        return Err("Runtime setup did not pass verification. Retry repair.".into());
    }

    #[cfg(target_os = "macos")]
    {
        if !status.mps_available {
            return Err("Model files are installed, but PyTorch could not access Metal. Restart Sculpt and verify the runtime.".into());
        }
    }

    #[cfg(not(target_os = "macos"))]
    {
        // On non-macOS platforms, MPS is not applicable.
        // Device acceleration is verified via manifest inspection if available.
        let manifest: Value = std::fs::read(root.join("ready.json"))
            .ok()
            .and_then(|v| serde_json::from_slice(&v).ok())
            .unwrap_or(Value::Null);
        let _cuda = manifest["cudaAvailable"].as_bool().unwrap_or(false);
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
    validate_setup_hardware(&hardware)?;

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

#[tauri::command]
pub async fn repair_runtime(
    app: AppHandle,
    harness: State<'_, SculptInferenceHarness>,
    job_id: String,
) -> Result<python::BackendStatus, String> {
    uuid::Uuid::parse_str(&job_id).map_err(|_| "Invalid setup identifier")?;
    let root = paths::runtime_dir(&app)?;
    let jobs = harness.jobs.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let guard = jobs.lock().map_err(|_| "Job state unavailable")?;
        if !guard.is_empty() {
            return Err("Finish or cancel the active operation before repair.".into());
        }
        uninstall_runtime_files(&root, true)
    })
    .await
    .map_err(|e| e.to_string())??;

    install_runtime(app, harness, job_id).await
}

#[tauri::command]
pub async fn uninstall_runtime(
    app: AppHandle,
    harness: State<'_, SculptInferenceHarness>,
) -> Result<python::BackendStatus, String> {
    let root = paths::runtime_dir(&app)?;
    let jobs = harness.jobs.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let guard = jobs.lock().map_err(|_| "Job state unavailable")?;
        if !guard.is_empty() {
            return Err(
                "Finish or cancel the active operation before uninstalling the runtime.".into(),
            );
        }
        uninstall_runtime_files(&root, false)?;
        Ok(python::inspect(&root))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uv_platform_resolution_matrix() {
        // macOS Apple Silicon (ARM64)
        let mac_arm = uv_release_for("macos", "arm64").unwrap();
        assert_eq!(mac_arm.archive_filename, "uv-aarch64-apple-darwin.tar.gz");
        assert_eq!(
            mac_arm.sha256,
            "8a09f0ef51ee7f7170731b4cb8bde5bf9ba6da5304f49a7df6cdab42a1f37b5d"
        );
        assert!(!mac_arm.is_zip);
        assert_eq!(mac_arm.binary_name, "uv");

        // macOS Intel (x86_64)
        let mac_intel = uv_release_for("darwin", "x86_64").unwrap();
        assert_eq!(mac_intel.archive_filename, "uv-x86_64-apple-darwin.tar.gz");
        assert_eq!(
            mac_intel.sha256,
            "dd18420591d625f9b4ca2b57a7a6fe3cce43910f02e02d90e47a4101428de14a"
        );

        // Linux x86_64
        let linux_x64 = uv_release_for("linux", "x86_64").unwrap();
        assert_eq!(
            linux_x64.archive_filename,
            "uv-x86_64-unknown-linux-gnu.tar.gz"
        );
        assert_eq!(
            linux_x64.sha256,
            "3e1027f26ce8c7e4c32e2277a7fed2cb410f2f1f9320d3df97653d40e21f415b"
        );

        // Linux ARM64
        let linux_arm = uv_release_for("linux", "aarch64").unwrap();
        assert_eq!(
            linux_arm.archive_filename,
            "uv-aarch64-unknown-linux-gnu.tar.gz"
        );
        assert_eq!(
            linux_arm.sha256,
            "2b80457b950deda12e8d5dc3b9b7494ac143eae47f1fb11b1c6e5a8495a6421e"
        );

        // Windows x64
        let win_x64 = uv_release_for("windows", "x86_64").unwrap();
        assert_eq!(win_x64.archive_filename, "uv-x86_64-pc-windows-msvc.zip");
        assert_eq!(
            win_x64.sha256,
            "d31a30f1dfb96e630a08d5a9b3f3f551254b7ed6e9b7e495f46a4232661c7252"
        );
        assert!(win_x64.is_zip);
        assert_eq!(win_x64.binary_name, "uv.exe");

        // Windows ARM64
        let win_arm = uv_release_for("windows", "arm64").unwrap();
        assert_eq!(win_arm.archive_filename, "uv-aarch64-pc-windows-msvc.zip");
        assert_eq!(
            win_arm.sha256,
            "2c6fe113f14574bc27f085751c68d3485589fcc3c3c64ed85dd1eecc2f87cffc"
        );
        assert!(win_arm.is_zip);
        assert_eq!(win_arm.binary_name, "uv.exe");

        // Unsupported combinations fail cleanly
        assert!(uv_release_for("freebsd", "x86_64").is_err());
        assert!(uv_release_for("windows", "mips").is_err());
    }

    #[test]
    fn archive_path_traversal_detection() {
        // Safe relative paths pass
        assert!(validate_archive_entries(&["uv", "uv.exe", "uv-x86_64/uv.exe"]).is_ok());
        assert!(validate_archive_entries(&["folder/subfolder/file.dat"]).is_ok());

        // Parent directory traversal is rejected
        assert!(validate_archive_entries(&["../evil.exe"]).is_err());
        assert!(validate_archive_entries(&["folder/../../evil.exe"]).is_err());
        assert!(validate_archive_entries(&["a/b/../c/../../d"]).is_err());

        // Absolute paths (Unix & Windows) are rejected
        assert!(validate_archive_entries(&["/usr/bin/evil"]).is_err());
        assert!(validate_archive_entries(&["/etc/shadow"]).is_err());
        assert!(validate_archive_entries(&["\\Windows\\evil.exe"]).is_err());
        assert!(validate_archive_entries(&["C:\\evil.exe"]).is_err());
        assert!(validate_archive_entries(&["C:/evil.exe"]).is_err());
        assert!(validate_archive_entries(&["D:relative_evil.exe"]).is_err());
    }

    #[test]
    fn executable_lookup_finds_system_commands() {
        // Standard commands exist across platforms
        assert!(find_executable("curl").is_some());
        assert!(find_executable("tar").is_some());
        assert!(find_executable("non_existent_command_xyz_987").is_none());
    }

    #[test]
    fn hardware_preflight_validation() {
        let mut profile = super::super::hardware::HardwareProfile {
            os: "windows".into(),
            os_version: "11".into(),
            architecture: "x86_64".into(),
            chip: "AMD Ryzen 9".into(),
            gpu: "RTX 4090".into(),
            memory_gb: 32.0,
            unified_memory: false,
            is_apple_silicon: false,
            compute_backends: vec!["CUDA".into()],
            detection_source: "native".into(),
            storage_gb: Some(100.0),
            npu: None,
        };

        // Sufficient Windows hardware passes
        assert!(validate_setup_hardware(&profile).is_ok());

        // Low RAM (< 8GB) fails with truthful message
        profile.memory_gb = 4.0;
        let err = validate_setup_hardware(&profile).unwrap_err();
        assert!(err.contains("at least 8 GB"));
        assert!(err.contains("4.0 GB"));

        // Low disk space (< 5GB) fails
        profile.memory_gb = 16.0;
        profile.storage_gb = Some(2.0);
        let err = validate_setup_hardware(&profile).unwrap_err();
        assert!(err.contains("At least 5 GB of free disk space"));

        // macOS non-Apple Silicon fails
        profile.storage_gb = Some(50.0);
        profile.os = "macos".into();
        profile.is_apple_silicon = false;
        let err = validate_setup_hardware(&profile).unwrap_err();
        assert!(err.contains("requires an Apple Silicon Mac"));

        // macOS Apple Silicon with 16GB passes
        profile.is_apple_silicon = true;
        assert!(validate_setup_hardware(&profile).is_ok());
    }

    #[test]
    fn runtime_uninstall_and_repair_lifecycle() {
        let root =
            std::env::temp_dir().join(format!("sculpt_test_lifecycle_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join("models/triposr")).unwrap();
        std::fs::create_dir_all(root.join("package-cache")).unwrap();
        std::fs::create_dir_all(root.join("venv/Scripts")).unwrap();
        std::fs::create_dir_all(root.join("tools")).unwrap();
        std::fs::create_dir_all(root.join("TripoSR")).unwrap();

        std::fs::write(root.join("ready.json"), b"{\"version\":1}").unwrap();
        std::fs::write(root.join("models/triposr/model.ckpt"), b"mock weights").unwrap();
        std::fs::write(root.join("package-cache/cached.whl"), b"mock wheel").unwrap();
        std::fs::write(root.join("tools/uv.exe"), b"mock uv").unwrap();
        std::fs::write(root.join("venv/Scripts/python.exe"), b"mock py").unwrap();

        assert!(root.join("ready.json").is_file());
        assert!(root.join("models/triposr/model.ckpt").is_file());

        // Test repair mode (keep_models = true): clears venv/ready.json but retains models & caches
        uninstall_runtime_files(&root, true).unwrap();
        assert!(!root.join("ready.json").exists());
        assert!(!root.join("venv").exists());
        assert!(!root.join("TripoSR").exists());
        assert!(root.join("models/triposr/model.ckpt").is_file());
        assert!(root.join("package-cache/cached.whl").is_file());

        // Test full uninstall (keep_models = false): clears all models, tools, and caches
        uninstall_runtime_files(&root, false).unwrap();
        assert!(!root.join("models").exists());
        assert!(!root.join("tools").exists());
        assert!(!root.join("package-cache").exists());

        let _ = std::fs::remove_dir_all(&root);
    }

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
