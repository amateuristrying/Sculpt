//! Foreground masks are immutable, source-bound artifacts. Preparing or editing
//! one does not create a generation job or consume the trial.
use super::{library::Library, paths, process, runtime::JobContext, SculptInferenceHarness};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    io::Cursor,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};
use tauri::{AppHandle, Emitter, State};

const MAX_PNG: usize = 5 * 1024 * 1024;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MaskPreview {
    source_id: String,
    sha256: String,
    width: u32,
    height: u32,
    image_data_url: String,
    mask_data_url: String,
}

pub fn validate_hash(hash: &str) -> Result<(), String> {
    if hash.len() != 64
        || !hash
            .bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
    {
        return Err("Invalid foreground mask identifier".into());
    }
    Ok(())
}

fn read_png(path: &Path) -> Result<Vec<u8>, String> {
    let meta = std::fs::symlink_metadata(path)
        .map_err(|_| "The saved mask preview is missing. Prepare it again.")?;
    if !meta.is_file() || meta.is_symlink() || meta.len() > MAX_PNG as u64 {
        return Err("Invalid mask file".into());
    }
    std::fs::read(path).map_err(|e| e.to_string())
}

fn decode_png(bytes: &[u8]) -> Result<image::DynamicImage, String> {
    if bytes.len() > MAX_PNG || image::guess_format(bytes).ok() != Some(image::ImageFormat::Png) {
        return Err("Masks must be PNG images smaller than 5 MB".into());
    }
    let reader = image::ImageReader::with_format(Cursor::new(bytes), image::ImageFormat::Png);
    let (width, height) = reader.into_dimensions().map_err(|e| e.to_string())?;
    if width == 0 || height == 0 || width > 1024 || height > 1024 {
        return Err("Mask dimensions exceed the preview size".into());
    }
    image::load_from_memory_with_format(bytes, image::ImageFormat::Png).map_err(|e| e.to_string())
}

pub fn mask_path(library: &Library, source_id: &str, hash: &str) -> Result<PathBuf, String> {
    validate_hash(hash)?;
    let directory = library.mask_directory(source_id)?;
    let path = directory.join(format!("{hash}.png"));
    let bytes = read_png(&path)?;
    if format!("{:x}", Sha256::digest(&bytes)) != hash {
        return Err("The saved foreground mask changed or is damaged. Review it again.".into());
    }
    Ok(path)
}

fn preview(directory: &Path, source_id: String, hash: String) -> Result<MaskPreview, String> {
    validate_hash(&hash)?;
    let image = read_png(&directory.join("preview.png"))?;
    let mask = read_png(&directory.join(format!("{hash}.png")))?;
    if format!("{:x}", Sha256::digest(&mask)) != hash {
        return Err("The saved foreground mask is damaged".into());
    }
    let decoded = decode_png(&mask)?;
    Ok(MaskPreview {
        source_id,
        sha256: hash,
        width: decoded.width(),
        height: decoded.height(),
        image_data_url: format!("data:image/png;base64,{}", STANDARD.encode(image)),
        mask_data_url: format!("data:image/png;base64,{}", STANDARD.encode(mask)),
    })
}

fn publish(directory: &Path, name: &str, bytes: &[u8]) -> Result<(), String> {
    let temporary = directory.join(format!(".{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| {
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(|e| e.to_string())?;
        file.write_all(bytes).map_err(|e| e.to_string())?;
        file.sync_all().map_err(|e| e.to_string())?;
        std::fs::rename(&temporary, directory.join(name)).map_err(|e| e.to_string())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(temporary);
    }
    result
}

fn store_mask(directory: &Path, bytes: &[u8]) -> Result<String, String> {
    let image = decode_png(&read_png(&directory.join("preview.png"))?)?;
    let mask = decode_png(bytes)?;
    if (image.width(), image.height()) != (mask.width(), mask.height()) {
        return Err("The mask does not match the source preview dimensions".into());
    }
    let gray = mask.to_luma8();
    if gray.pixels().filter(|p| p.0[0] > 32).count() < 64 {
        return Err("Keep at least part of the object in the mask before saving".into());
    }
    let mut encoded = Cursor::new(Vec::new());
    gray.write_to(&mut encoded, image::ImageFormat::Png)
        .map_err(|e| e.to_string())?;
    let bytes = encoded.into_inner();
    let hash = format!("{:x}", Sha256::digest(&bytes));
    publish(directory, &format!("{hash}.png"), &bytes)?;
    Ok(hash)
}

#[tauri::command]
pub async fn save_mask(
    harness: State<'_, SculptInferenceHarness>,
    source_id: String,
    data_url: String,
) -> Result<MaskPreview, String> {
    if data_url.len() > MAX_PNG * 4 / 3 + 128 {
        return Err("Mask exceeds its size limit".into());
    }
    let encoded = data_url
        .strip_prefix("data:image/png;base64,")
        .ok_or("Use a PNG mask")?;
    let bytes = STANDARD
        .decode(encoded)
        .map_err(|_| "Invalid mask encoding")?;
    harness
        .library_work(move |library| {
            let directory = library.mask_directory(&source_id)?;
            let hash = store_mask(&directory, &bytes)?;
            preview(&directory, source_id, hash)
        })
        .await
}

#[tauri::command]
pub async fn read_mask(
    harness: State<'_, SculptInferenceHarness>,
    source_id: String,
    sha256: String,
) -> Result<MaskPreview, String> {
    harness
        .library_work(move |library| {
            preview(&library.mask_directory(&source_id)?, source_id, sha256)
        })
        .await
}

// The blocking task owns both reservation and scratch files even if IPC disconnects.
struct MaskOperation {
    id: String,
    jobs: Arc<Mutex<HashMap<String, Arc<AtomicBool>>>>,
    scratch: PathBuf,
}
impl Drop for MaskOperation {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.scratch);
        if let Ok(mut jobs) = self.jobs.lock() {
            jobs.remove(&self.id);
        }
    }
}

#[tauri::command]
pub async fn prepare_mask(
    app: AppHandle,
    harness: State<'_, SculptInferenceHarness>,
    source_id: String,
    background: super::runtime::BackgroundMode,
    job_id: String,
) -> Result<MaskPreview, String> {
    if !uuid::Uuid::parse_str(&job_id).is_ok_and(|id| id.to_string() == job_id) {
        return Err("Invalid mask job identifier".into());
    }
    let lookup = source_id.clone();
    let (source, directory) = harness
        .library_work(move |library| {
            Ok((library.source(&lookup)?, library.mask_directory(&lookup)?))
        })
        .await?;
    let root = paths::runtime_dir(&app)?;
    let worker = paths::backend_file(&app, "worker.py")?;
    let cancelled = Arc::new(AtomicBool::new(false));
    {
        let mut jobs = harness.jobs.lock().map_err(|_| "Job state unavailable")?;
        if !jobs.is_empty() {
            return Err("Finish or cancel the active operation before preparing a mask.".into());
        }
        jobs.insert(job_id.clone(), cancelled.clone());
    }
    let operation = MaskOperation {
        id: job_id.clone(),
        jobs: harness.jobs.clone(),
        scratch: directory.join(format!(".mask-{job_id}")),
    };
    let context = JobContext {
        id: job_id,
        cancelled: cancelled.clone(),
        progress: Arc::new(move |event| {
            let _ = app.emit_to("main", "sculpt://generation-progress", event);
        }),
    };
    let _cancel_on_disconnect = super::CancelOnDrop(cancelled);
    tauri::async_runtime::spawn_blocking(move || {
        let _operation = operation;
        run_mask_worker(
            &root,
            &worker,
            &source.path,
            &source.asset.sha256,
            &directory,
            &_operation.scratch,
            background,
            &context,
        )?;
        if context.cancelled.load(Ordering::Relaxed) {
            return Err("Mask preparation cancelled".into());
        }
        let bytes = read_png(&_operation.scratch.join("result/mask.png"))?;
        let image = read_png(&_operation.scratch.join("result/preview.png"))?;
        decode_png(&image)?;
        publish(&directory, "preview.png", &image)?;
        // An empty automatic prediction is editable: return it, but save_mask
        // and generation still reject empty foregrounds.
        let hash = format!("{:x}", Sha256::digest(&bytes));
        publish(&directory, &format!("{hash}.png"), &bytes)?;
        preview(&directory, source_id, hash)
    })
    .await
    .map_err(|e| e.to_string())?
}

fn run_mask_worker(
    root: &Path,
    worker: &Path,
    source: &Path,
    source_sha: &str,
    directory: &Path,
    scratch: &Path,
    background: super::runtime::BackgroundMode,
    context: &JobContext,
) -> Result<(), String> {
    if context.cancelled.load(Ordering::Relaxed) {
        return Err("Mask preparation cancelled".into());
    }
    std::fs::create_dir(scratch).map_err(|e| e.to_string())?;
    let request = scratch.join("request.json");
    std::fs::write(
        &request,
        serde_json::to_vec(&serde_json::json!({"operation":"mask", "sourcePath":source,
        "sourceSha256":source_sha, "outputPath":scratch.join("result"), "background":background}))
        .map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    let log = directory.join("mask-worker.log");
    let mut command = Command::new(root.join("venv/bin/python"));
    command
        .arg(worker)
        .arg("--request")
        .arg(request)
        .env("SCULPT_RUNTIME_DIR", root)
        .env("OMP_NUM_THREADS", "4")
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .stderr(Stdio::from(
            std::fs::File::create(&log).map_err(|e| e.to_string())?,
        ));
    context.emit("analyzing", 1.0, "Preparing the foreground mask");
    let mut complete = false;
    process::run(
        &mut command,
        &context.cancelled,
        Duration::from_secs(180),
        |line| {
            let event: serde_json::Value =
                serde_json::from_str(line).map_err(|_| "Invalid mask worker message")?;
            if event["protocol"] != 1 {
                return Err("Unsupported mask worker protocol".into());
            }
            match event["type"].as_str() {
                Some("progress") => context.emit(
                    "analyzing",
                    event["progress"].as_f64().unwrap_or(0.0).clamp(0.0, 99.0),
                    event["message"].as_str().unwrap_or("Preparing mask"),
                ),
                Some("result") if !complete => complete = true,
                Some("error") => {
                    return Err(event["message"]
                        .as_str()
                        .unwrap_or("Mask preparation failed")
                        .into())
                }
                _ => return Err("Unexpected mask worker response".into()),
            }
            Ok(())
        },
    )
    .map_err(|e| format!("{e} Details: {}", log.display()))?;
    if !complete {
        return Err("Mask worker exited without a result".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (Library, String, PathBuf) {
        let root = std::env::temp_dir().join(format!("sculpt-mask-{}", uuid::Uuid::new_v4()));
        let mut library = Library::open(root).unwrap();
        let mut bytes = Cursor::new(Vec::new());
        image::RgbImage::from_pixel(64, 32, image::Rgb([240, 200, 20]))
            .write_to(&mut bytes, image::ImageFormat::Png)
            .unwrap();
        let source = super::super::assets::decode_source(
            "source.png".into(),
            &format!("data:image/png;base64,{}", STANDARD.encode(bytes.get_ref())),
            &library.source_directory().unwrap(),
        )
        .unwrap();
        let id = source.asset.id.clone();
        library.register_source(source).unwrap();
        let directory = library.mask_directory(&id).unwrap();
        publish(&directory, "preview.png", bytes.get_ref()).unwrap();
        (library, id, directory)
    }
    fn png(width: u32, height: u32, value: u8) -> Vec<u8> {
        let mut bytes = Cursor::new(Vec::new());
        image::GrayImage::from_pixel(width, height, image::Luma([value]))
            .write_to(&mut bytes, image::ImageFormat::Png)
            .unwrap();
        bytes.into_inner()
    }
    #[test]
    fn masks_are_source_bound_and_immutable_without_trial_use() {
        let (library, source_id, directory) = fixture();
        let first = store_mask(&directory, &png(64, 32, 255)).unwrap();
        let second = store_mask(&directory, &png(64, 32, 192)).unwrap();
        assert_ne!(first, second);
        assert!(mask_path(&library, &source_id, &first).unwrap().is_file());
        assert!(mask_path(&library, &uuid::Uuid::new_v4().to_string(), &first).is_err());
        assert!(library.trial_success_job().is_none());
        assert!(library.list_jobs(50).is_empty());
        std::fs::write(directory.join(format!("{first}.png")), png(64, 32, 0)).unwrap();
        assert!(mask_path(&library, &source_id, &first)
            .unwrap_err()
            .contains("damaged"));
    }
    #[test]
    fn saving_rejects_empty_foreground_and_wrong_coordinates() {
        let (_, _, directory) = fixture();
        assert!(store_mask(&directory, &png(32, 64, 255))
            .unwrap_err()
            .contains("dimensions"));
        assert!(store_mask(&directory, &png(64, 32, 0))
            .unwrap_err()
            .contains("object"));
        assert!(decode_png(&png(1025, 32, 255)).is_err());
        assert!(validate_hash("../preview").is_err());
    }
}

#[cfg(test)]
mod integration_tests {
    use super::*;
    #[test]
    #[ignore = "requires the installed local runtime and SCULPT_TEST_IMAGE"]
    fn real_mask_runs_through_native_supervision_without_spending_trial() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join(".sculpt-runtime");
        let source = PathBuf::from(std::env::var("SCULPT_TEST_IMAGE").expect("SCULPT_TEST_IMAGE"));
        let bytes = std::fs::read(&source).unwrap();
        let mut library = Library::open(
            root.join("test-output")
                .join(uuid::Uuid::new_v4().to_string()),
        )
        .unwrap();
        let record = super::super::assets::decode_source(
            "test.jpg".into(),
            &format!("data:image/jpeg;base64,{}", STANDARD.encode(bytes)),
            &library.source_directory().unwrap(),
        )
        .unwrap();
        library.register_source(record.clone()).unwrap();
        let directory = library.mask_directory(&record.asset.id).unwrap();
        let scratch = directory.join("native-test");
        let context = JobContext {
            id: uuid::Uuid::new_v4().to_string(),
            cancelled: Arc::new(AtomicBool::new(false)),
            progress: Arc::new(|p| println!("Mask: {}", p.message)),
        };
        run_mask_worker(
            &root,
            &root.parent().unwrap().join("backend/worker.py"),
            &record.path,
            &record.asset.sha256,
            &directory,
            &scratch,
            super::super::runtime::BackgroundMode::Auto,
            &context,
        )
        .unwrap();
        publish(
            &directory,
            "preview.png",
            &read_png(&scratch.join("result/preview.png")).unwrap(),
        )
        .unwrap();
        let hash = store_mask(
            &directory,
            &read_png(&scratch.join("result/mask.png")).unwrap(),
        )
        .unwrap();
        assert!(mask_path(&library, &record.asset.id, &hash)
            .unwrap()
            .is_file());
        assert!(library.trial_success_job().is_none());
        assert!(library.list_jobs(50).is_empty());
        println!("Verified source-bound native mask: {hash}");
    }
}
