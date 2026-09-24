pub mod assets;
pub mod cache;
pub mod catalog;
pub mod engines;
pub mod hardware;
pub mod library;
pub mod licensing;
pub mod masks;
pub mod paths;
pub mod process;
pub mod python;
pub mod runtime;
pub mod setup;

use runtime::{GeneratedAsset, GenerationRequest, InferenceRuntime, JobContext, MockRuntime};
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
};
use tauri::{AppHandle, Emitter, State};

/// Owns job lifetime, compatibility, cancellation and runtime selection. Future model
/// download/load/fallback policies belong here, never inside the React workspace.
pub struct SculptInferenceHarness {
    jobs: Arc<Mutex<HashMap<String, Arc<AtomicBool>>>>,
    library: Arc<Mutex<Result<library::Library, String>>>,
    license_policy: licensing::LicensePolicy,
}

impl SculptInferenceHarness {
    pub fn new(root: Result<std::path::PathBuf, String>) -> Self {
        Self {
            jobs: Arc::new(Mutex::new(HashMap::new())),
            library: Arc::new(Mutex::new(root.and_then(library::Library::open))),
            license_policy: licensing::LicensePolicy::configured(),
        }
    }

    // Filesystem work never blocks Tauri's async executor or the window thread.
    async fn library_work<T: Send + 'static>(
        &self,
        work: impl FnOnce(&mut library::Library) -> Result<T, String> + Send + 'static,
    ) -> Result<T, String> {
        let library = self.library.clone();
        tauri::async_runtime::spawn_blocking(move || with_library(&library, work))
            .await
            .map_err(|error| error.to_string())?
    }
}

fn with_library<T>(
    state: &Mutex<Result<library::Library, String>>,
    work: impl FnOnce(&mut library::Library) -> Result<T, String>,
) -> Result<T, String> {
    let mut guard = state.lock().map_err(|_| "Local library unavailable")?;
    let library = guard
        .as_mut()
        .map_err(|error| format!("Local library unavailable: {error}"))?;
    work(library)
}

/// The detached task owns the reservation until the worker has stopped and the
/// terminal state is durable. Dropping an IPC request only requests cancellation.
struct JobReservation {
    id: String,
    jobs: Arc<Mutex<HashMap<String, Arc<AtomicBool>>>>,
    library: Arc<Mutex<Result<library::Library, String>>>,
    cancelled: Arc<AtomicBool>,
    finalized: bool,
}

impl JobReservation {
    fn finish(&mut self, result: Result<GeneratedAsset, String>) -> Result<GeneratedAsset, String> {
        // This is the commit boundary. Cancellation either wins before the commit
        // or observes an already saved result; it cannot undo a completed trial.
        let mut jobs = self.jobs.lock().map_err(|_| "Job state unavailable")?;
        let was_cancelled = self.cancelled.load(Ordering::Relaxed);
        let result = if was_cancelled {
            Err("Generation cancelled".into())
        } else {
            result
        };
        with_library(&self.library, |library| {
            library.finish_job(&self.id, result.clone(), was_cancelled)
        })?;
        self.finalized = true;
        jobs.remove(&self.id);
        result
    }
}

impl Drop for JobReservation {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Relaxed);
        if !self.finalized {
            let _ = with_library(&self.library, |library| {
                library.finish_job(
                &self.id, Err("Generation interrupted before the result was saved. You can retry this source.".into()), false,
            )
            });
        }
        if let Ok(mut jobs) = self.jobs.lock() {
            jobs.remove(&self.id);
        }
    }
}

struct CancelOnDrop(Arc<AtomicBool>);
impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Relaxed);
    }
}

#[tauri::command]
pub async fn list_generation_jobs(
    harness: State<'_, SculptInferenceHarness>,
    limit: Option<usize>,
) -> Result<Vec<library::JobRecord>, String> {
    harness
        .library_work(move |library| Ok(library.list_jobs(limit.unwrap_or(50).clamp(1, 100))))
        .await
}

#[tauri::command]
pub async fn get_access_status(
    harness: State<'_, SculptInferenceHarness>,
) -> Result<licensing::AccessStatus, String> {
    let policy = licensing::LicensePolicy::configured();
    harness
        .library_work(move |library| {
            Ok(licensing::access_status(
                &policy,
                library.signed_license(),
                library.trial_success_job().is_some(),
            ))
        })
        .await
}

#[tauri::command]
pub async fn activate_license(
    harness: State<'_, SculptInferenceHarness>,
    signed_license: String,
) -> Result<licensing::AccessStatus, String> {
    let policy = licensing::LicensePolicy::configured();
    harness
        .library_work(move |library| {
            licensing::validate_activation(&policy, &signed_license)?;
            library.set_signed_license(Some(signed_license))?;
            Ok(licensing::access_status(
                &policy,
                library.signed_license(),
                library.trial_success_job().is_some(),
            ))
        })
        .await
}

#[tauri::command]
pub async fn detect_hardware() -> Result<hardware::HardwareProfile, String> {
    tauri::async_runtime::spawn_blocking(hardware::detect)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub fn get_engines(profile: hardware::HardwareProfile) -> Vec<catalog::EngineProfile> {
    catalog::engines(&profile)
}

#[tauri::command]
pub async fn generate_asset(
    app: AppHandle,
    harness: State<'_, SculptInferenceHarness>,
    request: GenerationRequest,
    job_id: String,
) -> Result<GeneratedAsset, String> {
    if request.parent_asset_id.is_some() || request.refinement.is_some() {
        return Err("Use the native refinement command for saved assets.".into());
    }
    run_asset(app, harness, request, job_id).await
}

#[tauri::command]
pub async fn refine_asset(
    app: AppHandle,
    harness: State<'_, SculptInferenceHarness>,
    parent_asset_id: String,
    settings: runtime::RefinementSettings,
    job_id: String,
) -> Result<GeneratedAsset, String> {
    settings.validate()?;
    let request = harness
        .library_work(move |library| {
            // A refinement always starts from a completed, verified local asset.
            library.asset_path(&parent_asset_id)?;
            let parent = library
                .job(&parent_asset_id)
                .ok_or("Unknown parent asset")?;
            let mut request = parent.request;
            request.parent_asset_id = Some(parent_asset_id);
            request.refinement = Some(settings);
            Ok(request)
        })
        .await?;
    run_asset(app, harness, request, job_id).await
}

async fn run_asset(
    app: AppHandle,
    harness: State<'_, SculptInferenceHarness>,
    request: GenerationRequest,
    job_id: String,
) -> Result<GeneratedAsset, String> {
    if !uuid::Uuid::parse_str(&job_id).is_ok_and(|id| id.to_string() == job_id) {
        return Err("Invalid job identifier".into());
    }
    if request.image_name.trim().is_empty() {
        return Err("Select a source image first".into());
    }
    let profile = tauri::async_runtime::spawn_blocking(hardware::detect)
        .await
        .map_err(|error| error.to_string())?;
    let selection = engines::resolve(&request.engine_id, &profile)?;
    let output_id = job_id.clone();
    let output_dir = harness
        .library_work(move |library| library.job_directory(&output_id))
        .await?;
    let runtime: Box<dyn InferenceRuntime> = match selection.adapter {
        engines::RuntimeAdapter::TripoSr => {
            let source_id = request
                .source_id
                .clone()
                .ok_or("Import an image before generating")?;
            let source = harness
                .library_work(move |library| library.source(&source_id))
                .await?;
            let worker = paths::backend_file(&app, "worker.py")?;
            let parent_id = request.parent_asset_id.clone();
            let scene_cache = if let Some(parent_id) = parent_id {
                Some(
                    harness
                        .library_work(move |library| verified_scene_cache(library, &parent_id))
                        .await?,
                )
            } else {
                None
            };
            Box::new(python::PythonRuntime {
                root: paths::runtime_dir(&app)?,
                worker,
                source: source.path,
                source_sha256: source.asset.sha256,
                output_dir: output_dir.clone(),
                scene_cache,
                device: selection.device.as_str().into(),
                mask: if let Some(hash) = request.mask_sha256.clone() {
                    let source_id = source.asset.id.clone();
                    Some(
                        harness
                            .library_work(move |library| {
                                masks::mask_path(library, &source_id, &hash)
                            })
                            .await?,
                    )
                } else {
                    None
                },
            })
        }
        engines::RuntimeAdapter::Demo => Box::new(MockRuntime),
    };
    let cancelled = Arc::new(AtomicBool::new(false));
    let job_registry = harness.jobs.clone();
    let library = harness.library.clone();
    let policy = harness.license_policy.clone();
    let admitted_id = job_id.clone();
    let admitted_request = request.clone();
    let admitted_cancelled = cancelled.clone();
    let reservation = tauri::async_runtime::spawn_blocking(move || {
        let mut jobs = job_registry.lock().map_err(|_| "Job state unavailable")?;
        if jobs.contains_key(&admitted_id) {
            return Err("Job identifier already in use".into());
        }
        if !jobs.is_empty() {
            return Err(
                "A generation is already running. Cancel it before starting another.".into(),
            );
        }
        // Admission and trial authorization share the job lock. Two concurrent IPC
        // requests cannot both reserve the last free generation.
        with_library(&library, |library| {
            if admitted_request.refinement.is_none() {
                licensing::authorize_generation(
                    &policy,
                    library.signed_license(),
                    library.trial_success_job().is_some(),
                    admitted_request.engine_id == "demo",
                )?;
            }
            library.begin_job(&admitted_id, admitted_request)
        })?;
        jobs.insert(admitted_id.clone(), admitted_cancelled.clone());
        drop(jobs);
        Ok::<_, String>(JobReservation {
            id: admitted_id,
            jobs: job_registry,
            library,
            cancelled: admitted_cancelled,
            finalized: false,
        })
    })
    .await
    .map_err(|error| error.to_string())??;
    let _runtime_kind = runtime.kind();
    let event_app = app.clone();
    let context = JobContext {
        id: job_id.clone(),
        cancelled: cancelled.clone(),
        // A worker's completion is provisional until the native library commits it.
        progress: Arc::new(move |progress| {
            if progress.stage != "complete" {
                let _ = event_app.emit_to("main", "sculpt://generation-progress", progress);
            }
        }),
    };
    let _cancel_on_disconnect = CancelOnDrop(cancelled);
    tauri::async_runtime::spawn(async move {
        let result = runtime.generate(request, context).await;
        let committed = tauri::async_runtime::spawn_blocking(move || {
            let mut reservation = reservation;
            reservation.finish(result)
        })
        .await
        .map_err(|error| error.to_string())?;
        if committed.is_ok() {
            let _ = app.emit_to(
                "main",
                "sculpt://generation-progress",
                runtime::GenerationProgress {
                    job_id,
                    stage: "complete".into(),
                    progress: 100.0,
                    message: "Asset saved to your local library".into(),
                },
            );
        }
        committed
    })
    .await
    .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn cancel_generation(
    harness: State<'_, SculptInferenceHarness>,
    job_id: String,
) -> Result<(), String> {
    let jobs = harness.jobs.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let jobs = jobs.lock().map_err(|_| "Job state unavailable")?;
        if let Some(cancelled) = jobs.get(&job_id) {
            cancelled.store(true, Ordering::Relaxed);
        }
        Ok(())
    })
    .await
    .map_err(|error| error.to_string())?
}

fn verified_scene_cache(
    library: &library::Library,
    parent_id: &str,
) -> Result<std::path::PathBuf, String> {
    use sha2::{Digest, Sha256};
    use std::io::Read;
    let parent = library.job(parent_id).ok_or("Unknown parent asset")?;
    let expected = parent
        .asset
        .as_ref()
        .and_then(|asset| asset.metrics.as_ref())
        .and_then(|metrics| metrics["sceneCacheSha256"].as_str())
        .ok_or("This asset predates refinement. Generate it again to enable Refine.")?;
    let path = library.job_directory(parent_id)?.join("scene-cache.npz");
    let info = std::fs::symlink_metadata(&path)
        .map_err(|_| "The refinement cache is missing. Generate the image again.")?;
    if !info.is_file() || info.is_symlink() || info.len() > 4 * 1024 * 1024 {
        return Err("The refinement cache is invalid.".into());
    }
    let mut file = std::fs::File::open(&path).map_err(|e| e.to_string())?;
    let mut bytes = Vec::new();
    Read::by_ref(&mut file)
        .take(4 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > 4 * 1024 * 1024 || format!("{:x}", Sha256::digest(&bytes)) != expected {
        return Err("The refinement cache changed or is damaged. Generate the image again.".into());
    }
    Ok(path)
}

#[cfg(test)]
mod lifecycle_tests;
