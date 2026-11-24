use std::{
    fs,
    path::{Path, PathBuf},
};
use tauri::{AppHandle, Manager};

/// Development reconstruction never alters the release library.
pub fn library_dir(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app
        .path()
        .app_local_data_dir()
        .map_err(|e| e.to_string())?
        .join(if cfg!(debug_assertions) {
            "library-development"
        } else {
            "library"
        }))
}

pub fn runtime_dir(app: &AppHandle) -> Result<PathBuf, String> {
    if let Some(path) = std::env::var_os("SCULPT_RUNTIME_DIR") {
        return Ok(PathBuf::from(path));
    }
    if cfg!(debug_assertions) {
        return Ok(Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join(".sculpt-runtime"));
    }
    Ok(app
        .path()
        .app_local_data_dir()
        .map_err(|e| e.to_string())?
        .join("runtime"))
}

pub fn backend_file(app: &AppHandle, name: &str) -> Result<PathBuf, String> {
    let base = if cfg!(debug_assertions) {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .to_path_buf()
    } else {
        app.path().resource_dir().map_err(|e| e.to_string())?
    };
    Ok(base.join("backend").join(name))
}

/// Directory for cached package downloads and wheels inside the runtime root.
pub fn cache_dir(runtime_root: &Path) -> PathBuf {
    runtime_root.join("package-cache")
}

/// Directory for model weights and configs inside the runtime root.
pub fn models_dir(runtime_root: &Path) -> PathBuf {
    runtime_root.join("models")
}

/// Directory for job artifacts and outputs inside the library root.
pub fn jobs_dir(library_root: &Path) -> PathBuf {
    library_root.join("jobs")
}

/// Resolves the app-level cache directory via Tauri.
#[allow(dead_code)]
pub fn app_cache_dir(app: &AppHandle) -> Result<PathBuf, String> {
    app.path().app_cache_dir().map_err(|e| e.to_string())
}

/// Resolves the application log directory via Tauri (`SCULPT_LOG_DIR` overrides for tests).
pub fn log_dir(app: &AppHandle) -> Result<PathBuf, String> {
    if let Some(path) = std::env::var_os("SCULPT_LOG_DIR") {
        let candidate = PathBuf::from(path);
        if !candidate.as_os_str().is_empty() {
            return Ok(candidate);
        }
    }
    let base = app.path().app_log_dir().map_err(|e| e.to_string())?;
    Ok(log_dir_for_base(&base, cfg!(debug_assertions)))
}

/// Parameterized log directory resolver separating development and packaged logs.
pub fn log_dir_for_base(base: &Path, is_debug: bool) -> PathBuf {
    if is_debug {
        base.join("development")
    } else {
        base.to_path_buf()
    }
}

/// Resolves the runtime-level cache directory using the app handle.
#[allow(dead_code)]
pub fn runtime_cache_dir(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(cache_dir(&runtime_dir(app)?))
}

/// Resolves the runtime models directory using the app handle.
#[allow(dead_code)]
pub fn runtime_models_dir(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(models_dir(&runtime_dir(app)?))
}

/// Resolves the library jobs directory using the app handle.
#[allow(dead_code)]
pub fn library_jobs_dir(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(jobs_dir(&library_dir(app)?))
}

/// Resolves the platform-specific Python executable path within a virtual environment.
///
/// On Windows: `<runtime_root>/venv/Scripts/python.exe`
///             (falling back to `<runtime_root>/venv/python.exe` if present).
/// On Unix/macOS/Linux: `<runtime_root>/venv/bin/python`.
pub fn python_executable(runtime_root: &Path) -> PathBuf {
    python_executable_for_platform(runtime_root, cfg!(target_os = "windows"))
}

/// Parameterized Python executable resolver for deterministic multi-platform testing.
pub fn python_executable_for_platform(runtime_root: &Path, is_windows: bool) -> PathBuf {
    if is_windows {
        let scripts_exe = runtime_root.join("venv").join("Scripts").join("python.exe");
        if scripts_exe.exists() {
            scripts_exe
        } else {
            let venv_exe = runtime_root.join("venv").join("python.exe");
            if venv_exe.exists() {
                venv_exe
            } else {
                scripts_exe
            }
        }
    } else {
        runtime_root.join("venv").join("bin").join("python")
    }
}

/// Atomically replaces or creates `destination` with the contents of `temp`.
///
/// - Fails if `temp` does not exist or is not a regular file.
/// - Fails if `destination` is an existing directory.
/// - On Unix: uses `std::fs::rename`, which POSIX guarantees to be atomic for regular files on the same filesystem.
/// - On Windows: uses Win32 `MoveFileExW` with `MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH`
///   to ensure atomic replacement and write-through flush to disk.
pub fn atomic_replace(temp: &Path, destination: &Path) -> Result<(), String> {
    let temp_meta = fs::metadata(temp).map_err(|e| {
        format!(
            "Source temporary file '{}' does not exist or is inaccessible: {e}",
            temp.display()
        )
    })?;
    if !temp_meta.is_file() {
        return Err(format!(
            "Source temporary file '{}' is not a regular file",
            temp.display()
        ));
    }

    if let Ok(dest_meta) = fs::metadata(destination) {
        if dest_meta.is_dir() {
            return Err(format!(
                "Destination '{}' is a directory; atomic replacement requires a file destination",
                destination.display()
            ));
        }
    }

    #[cfg(unix)]
    {
        fs::rename(temp, destination).map_err(|e| {
            format!(
                "Failed to atomically rename '{}' to '{}': {e}",
                temp.display(),
                destination.display()
            )
        })
    }

    #[cfg(target_os = "windows")]
    {
        let temp_wide = win32::to_extended_wide_path(temp);
        let dest_wide = win32::to_extended_wide_path(destination);

        let res = unsafe {
            win32::MoveFileExW(
                temp_wide.as_ptr(),
                dest_wide.as_ptr(),
                win32::MOVEFILE_REPLACE_EXISTING | win32::MOVEFILE_WRITE_THROUGH,
            )
        };
        if res == 0 {
            let err = unsafe { win32::GetLastError() };
            return Err(format!(
                "Failed to atomically replace '{}' with '{}': Windows error code {}",
                destination.display(),
                temp.display(),
                err
            ));
        }
        Ok(())
    }

    #[cfg(not(any(unix, target_os = "windows")))]
    {
        fs::rename(temp, destination).map_err(|e| {
            format!(
                "Failed to rename '{}' to '{}': {e}",
                temp.display(),
                destination.display()
            )
        })
    }
}

/// Flushes directory metadata changes to durable storage across platforms.
///
/// - On Unix: opens the directory file descriptor and calls `File::sync_all()`.
/// - On Windows: directory handles cannot be opened with standard `File::open` (causes OS error 5 / Access Denied).
///   File operations using write-through or file-level syncing already guarantee NTFS journal updates,
///   so this validates that `dir` exists and is a valid directory without error.
pub fn sync_directory(dir: &Path) -> Result<(), String> {
    if !dir.exists() {
        return Err(format!("Directory '{}' does not exist", dir.display()));
    }
    let meta = fs::metadata(dir).map_err(|e| {
        format!(
            "Could not read directory metadata for '{}': {e}",
            dir.display()
        )
    })?;
    if !meta.is_dir() {
        return Err(format!("Path '{}' is not a directory", dir.display()));
    }

    #[cfg(unix)]
    {
        let file = fs::File::open(dir)
            .map_err(|e| format!("Could not open directory '{}' for sync: {e}", dir.display()))?;
        file.sync_all()
            .map_err(|e| format!("Could not sync directory '{}': {e}", dir.display()))?;
    }

    Ok(())
}

/// Validates that a path is relative and does not contain parent traversal ('..')
/// or root/prefix components.
#[allow(dead_code)]
pub fn validate_safe_relative_path(path: &Path) -> Result<(), String> {
    if path.is_absolute() {
        return Err(format!(
            "Path '{}' must be relative, not absolute",
            path.display()
        ));
    }
    for component in path.components() {
        match component {
            std::path::Component::ParentDir => {
                return Err(format!(
                    "Parent traversal ('..') is not permitted in '{}'",
                    path.display()
                ));
            }
            std::path::Component::Prefix(_) | std::path::Component::RootDir => {
                return Err(format!(
                    "Root or prefix components are not permitted in '{}'",
                    path.display()
                ));
            }
            _ => {}
        }
    }
    Ok(())
}

/// Strips the Windows verbatim `\\?\` prefix if the path is within standard MAX_PATH (260 characters)
/// and is not a verbatim UNC path (`\\?\UNC\`), keeping it clean for external tools while retaining
/// prefix safety for long paths.
pub fn strip_extended_prefix_if_safe(path: &Path) -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        let s = path.to_string_lossy();
        if s.starts_with(r"\\?\") && !s.starts_with(r"\\?\UNC\") && s.len() < 260 {
            return PathBuf::from(&s[4..]);
        }
    }
    path.to_path_buf()
}

/// Safely canonicalizes a path, stripping the verbatim prefix on Windows if within MAX_PATH length.
#[allow(dead_code)]
pub fn safe_canonicalize(path: &Path) -> Result<PathBuf, String> {
    let canonical = path
        .canonicalize()
        .map_err(|e| format!("Could not canonicalize path '{}': {e}", path.display()))?;
    Ok(strip_extended_prefix_if_safe(&canonical))
}

/// Opens a private regular file, setting mode 0600 (user-only read/write) on Unix platforms.
#[allow(dead_code)]
pub fn open_private_file(path: &Path, create: bool) -> Result<fs::File, String> {
    let mut options = fs::OpenOptions::new();
    options.read(true);
    if create {
        options.write(true).create(true);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    let file = options
        .open(path)
        .map_err(|e| format!("Could not open file '{}': {e}", path.display()))?;
    if !file
        .metadata()
        .map_err(|e| format!("Could not read metadata for '{}': {e}", path.display()))?
        .is_file()
    {
        return Err(format!("'{}' is not a regular file", path.display()));
    }
    Ok(file)
}

#[cfg(target_os = "windows")]
mod win32 {
    use std::{os::windows::ffi::OsStrExt, path::Path};

    pub const MOVEFILE_REPLACE_EXISTING: u32 = 0x00000001;
    pub const MOVEFILE_WRITE_THROUGH: u32 = 0x00000008;

    #[link(name = "kernel32")]
    extern "system" {
        pub fn MoveFileExW(
            lpExistingFileName: *const u16,
            lpNewFileName: *const u16,
            dwFlags: u32,
        ) -> i32;
        pub fn GetLastError() -> u32;
    }

    pub fn to_extended_wide_path(path: &Path) -> Vec<u16> {
        let s = path.to_string_lossy();
        if s.len() >= 240 && !s.starts_with(r"\\?\") {
            if let Ok(canonical) = path.canonicalize() {
                return canonical
                    .as_os_str()
                    .encode_wide()
                    .chain(std::iter::once(0))
                    .collect();
            }
        }
        path.as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_python_executable_resolution() {
        let dummy_root = Path::new("/dummy/runtime");

        // Unix resolution
        let unix_exe = python_executable_for_platform(dummy_root, false);
        assert_eq!(unix_exe, dummy_root.join("venv").join("bin").join("python"));

        // Windows resolution without existing files (defaults to Scripts/python.exe)
        let win_exe = python_executable_for_platform(dummy_root, true);
        assert_eq!(
            win_exe,
            dummy_root.join("venv").join("Scripts").join("python.exe")
        );

        // Windows resolution with fallback to venv/python.exe when present
        let temp_dir =
            std::env::temp_dir().join(format!("sculpt-test-py-{}", uuid::Uuid::new_v4()));
        let venv_dir = temp_dir.join("venv");
        fs::create_dir_all(&venv_dir).unwrap();
        let fallback_exe = venv_dir.join("python.exe");
        fs::write(&fallback_exe, b"").unwrap();

        let resolved_fallback = python_executable_for_platform(&temp_dir, true);
        assert_eq!(resolved_fallback, fallback_exe);

        // Windows resolution preferring Scripts/python.exe when Scripts exists
        let scripts_dir = venv_dir.join("Scripts");
        fs::create_dir_all(&scripts_dir).unwrap();
        let scripts_exe = scripts_dir.join("python.exe");
        fs::write(&scripts_exe, b"").unwrap();

        let resolved_scripts = python_executable_for_platform(&temp_dir, true);
        assert_eq!(resolved_scripts, scripts_exe);

        let _ = fs::remove_dir_all(temp_dir);

        // Native resolution runs on current host platform without error
        let native_exe = python_executable(dummy_root);
        if cfg!(target_os = "windows") {
            assert!(native_exe.to_string_lossy().ends_with("python.exe"));
        } else {
            assert!(native_exe.to_string_lossy().ends_with("python"));
        }
    }

    #[test]
    fn test_atomic_replace_new_destination() {
        let temp_dir =
            std::env::temp_dir().join(format!("sculpt-test-atomic-new-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&temp_dir).unwrap();

        let temp_file = temp_dir.join("temp.txt");
        let dest_file = temp_dir.join("dest.txt");

        fs::write(&temp_file, b"brand-new-content").unwrap();
        assert!(temp_file.exists());
        assert!(!dest_file.exists());

        atomic_replace(&temp_file, &dest_file)
            .expect("Atomic replace to new destination should succeed");

        assert!(!temp_file.exists());
        assert!(dest_file.exists());
        assert_eq!(fs::read(&dest_file).unwrap(), b"brand-new-content");

        let _ = fs::remove_dir_all(temp_dir);
    }

    #[test]
    fn test_atomic_replace_existing_destination() {
        let temp_dir = std::env::temp_dir().join(format!(
            "sculpt-test-atomic-overwrite-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&temp_dir).unwrap();

        let temp_file = temp_dir.join("temp.txt");
        let dest_file = temp_dir.join("dest.txt");

        fs::write(&dest_file, b"initial-existing-content").unwrap();
        fs::write(&temp_file, b"overwritten-content").unwrap();

        assert!(dest_file.exists());
        assert!(temp_file.exists());

        atomic_replace(&temp_file, &dest_file)
            .expect("Atomic replace overwriting existing file should succeed");

        assert!(!temp_file.exists());
        assert!(dest_file.exists());
        assert_eq!(fs::read(&dest_file).unwrap(), b"overwritten-content");

        let _ = fs::remove_dir_all(temp_dir);
    }

    #[test]
    fn test_atomic_replace_rejects_missing_temp() {
        let temp_dir = std::env::temp_dir().join(format!(
            "sculpt-test-atomic-missing-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&temp_dir).unwrap();

        let non_existent_temp = temp_dir.join("does-not-exist.tmp");
        let dest_file = temp_dir.join("dest.txt");

        let result = atomic_replace(&non_existent_temp, &dest_file);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("does not exist"));

        let _ = fs::remove_dir_all(temp_dir);
    }

    #[test]
    fn test_atomic_replace_rejects_directory_destination() {
        let temp_dir = std::env::temp_dir().join(format!(
            "sculpt-test-atomic-dir-dest-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&temp_dir).unwrap();

        let temp_file = temp_dir.join("temp.txt");
        fs::write(&temp_file, b"some-content").unwrap();

        let dir_dest = temp_dir.join("sub-dir");
        fs::create_dir_all(&dir_dest).unwrap();

        let result = atomic_replace(&temp_file, &dir_dest);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("is a directory"));

        let _ = fs::remove_dir_all(temp_dir);
    }

    #[test]
    fn test_atomic_replace_rejects_directory_temp() {
        let temp_dir = std::env::temp_dir().join(format!(
            "sculpt-test-atomic-dir-temp-{}",
            uuid::Uuid::new_v4()
        ));
        fs::create_dir_all(&temp_dir).unwrap();

        let source_dir = temp_dir.join("sub-source");
        fs::create_dir_all(&source_dir).unwrap();

        let dest_file = temp_dir.join("dest.txt");

        let result = atomic_replace(&source_dir, &dest_file);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("not a regular file"));

        let _ = fs::remove_dir_all(temp_dir);
    }

    #[test]
    fn test_sync_directory_success() {
        let temp_dir =
            std::env::temp_dir().join(format!("sculpt-test-sync-dir-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&temp_dir).unwrap();

        let res = sync_directory(&temp_dir);
        assert!(
            res.is_ok(),
            "sync_directory on existing directory must succeed: {:?}",
            res
        );

        let _ = fs::remove_dir_all(temp_dir);
    }

    #[test]
    fn test_sync_directory_missing_or_file() {
        let temp_dir =
            std::env::temp_dir().join(format!("sculpt-test-sync-missing-{}", uuid::Uuid::new_v4()));

        // Non-existent directory
        assert!(sync_directory(&temp_dir).is_err());

        // Regular file instead of directory
        fs::create_dir_all(&temp_dir).unwrap();
        let regular_file = temp_dir.join("file.txt");
        fs::write(&regular_file, b"data").unwrap();

        let res = sync_directory(&regular_file);
        assert!(res.is_err());
        assert!(res.unwrap_err().contains("not a directory"));

        let _ = fs::remove_dir_all(temp_dir);
    }

    #[test]
    fn test_path_validation_rejects_traversal() {
        assert!(validate_safe_relative_path(Path::new("foo/../bar")).is_err());
        assert!(validate_safe_relative_path(Path::new("../secret")).is_err());
        assert!(validate_safe_relative_path(Path::new("a/b/../../c")).is_err());
        assert!(validate_safe_relative_path(Path::new("..")).is_err());

        #[cfg(target_os = "windows")]
        {
            assert!(validate_safe_relative_path(Path::new(r"C:\Windows\System32")).is_err());
            assert!(validate_safe_relative_path(Path::new(r"\absolute\path")).is_err());
        }
        #[cfg(not(target_os = "windows"))]
        {
            assert!(validate_safe_relative_path(Path::new("/etc/passwd")).is_err());
        }

        assert!(validate_safe_relative_path(Path::new("valid/relative/path.txt")).is_ok());
        assert!(validate_safe_relative_path(Path::new("mesh.glb")).is_ok());
        assert!(validate_safe_relative_path(Path::new("sub/folder/data.json")).is_ok());
    }

    #[test]
    fn test_centralized_application_paths() {
        let root = Path::new("/custom/root");
        assert_eq!(cache_dir(root), root.join("package-cache"));
        assert_eq!(models_dir(root), root.join("models"));
        assert_eq!(jobs_dir(root), root.join("jobs"));
        assert_eq!(
            log_dir_for_base(Path::new("/custom/logs"), false),
            PathBuf::from("/custom/logs")
        );
        assert_eq!(
            log_dir_for_base(Path::new("/custom/logs"), true),
            PathBuf::from("/custom/logs").join("development")
        );
    }

    #[test]
    fn test_strip_extended_prefix_if_safe() {
        let normal = Path::new(r"C:\test\path");
        assert_eq!(strip_extended_prefix_if_safe(normal), normal);

        #[cfg(target_os = "windows")]
        {
            let verbatim = Path::new(r"\\?\C:\test\path");
            assert_eq!(
                strip_extended_prefix_if_safe(verbatim),
                Path::new(r"C:\test\path")
            );

            let unc = Path::new(r"\\?\UNC\server\share\file");
            assert_eq!(strip_extended_prefix_if_safe(unc), unc);
        }
    }
}
