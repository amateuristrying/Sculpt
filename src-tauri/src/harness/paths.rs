use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager};

/// Development reconstruction and trial accounting never alter the release library.
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
