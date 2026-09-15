//! Only disposable package downloads are evicted. Model/config caches are required offline.
use super::{paths, python, SculptInferenceHarness};
use std::path::Path;

pub fn download_bytes(root: &Path) -> u64 {
    fn size(path: &Path) -> u64 {
        let Ok(metadata) = std::fs::symlink_metadata(path) else {
            return 0;
        };
        if metadata.is_symlink() {
            return 0;
        }
        if metadata.is_file() {
            return metadata.len();
        }
        std::fs::read_dir(path)
            .map(|entries| entries.flatten().map(|e| size(&e.path())).sum())
            .unwrap_or(0)
    }
    size(&root.join("package-cache"))
}

fn clear(root: &Path) -> Result<(), String> {
    let cache = root.join("package-cache");
    match std::fs::symlink_metadata(&cache) {
        Ok(info) if info.is_dir() && !info.is_symlink() => {
            std::fs::remove_dir_all(cache).map_err(|e| e.to_string())
        }
        Ok(_) => Err("The download cache is not a regular directory.".into()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}

#[tauri::command]
pub async fn clear_download_cache(
    app: tauri::AppHandle,
    harness: tauri::State<'_, SculptInferenceHarness>,
) -> Result<python::BackendStatus, String> {
    // Hold the job lock throughout deletion; setup/generation cannot race eviction.
    let root = paths::runtime_dir(&app)?;
    let jobs = harness.jobs.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let guard = jobs.lock().map_err(|_| "Job state unavailable")?;
        if !guard.is_empty() {
            return Err("Finish or cancel the active operation before clearing downloads.".into());
        }
        clear(&root)?;
        Ok(python::inspect(&root))
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn eviction_preserves_models_and_ignores_symlinks() {
        let root = std::env::temp_dir().join(uuid::Uuid::new_v4().to_string());
        std::fs::create_dir_all(root.join("models")).unwrap();
        std::fs::create_dir_all(root.join("package-cache/wheels")).unwrap();
        std::fs::write(root.join("models/weight"), b"model").unwrap();
        std::fs::write(root.join("package-cache/wheels/package"), b"abc").unwrap();
        assert_eq!(download_bytes(&root), 3);
        clear(&root).unwrap();
        assert_eq!(std::fs::read(root.join("models/weight")).unwrap(), b"model");
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(root.join("models"), root.join("package-cache")).unwrap();
            assert!(clear(&root).is_err());
            assert_eq!(download_bytes(&root), 0);
        }
        std::fs::remove_dir_all(root).unwrap();
    }
}
