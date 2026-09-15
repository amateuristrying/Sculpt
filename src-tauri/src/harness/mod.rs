pub mod assets;
pub mod cache;
pub mod catalog;
pub mod hardware;
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
use tauri::{AppHandle, Emitter, Manager, State};

/// Owns job lifetime, compatibility, cancellation and runtime selection. Future model
/// download/load/fallback policies belong here, never inside the React workspace.
#[derive(Default)]
pub struct SculptInferenceHarness {
    jobs: Arc<Mutex<HashMap<String, Arc<AtomicBool>>>>,
    sources: Mutex<HashMap<String, assets::SourceRecord>>,
    outputs: Mutex<HashMap<String, std::path::PathBuf>>,
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
    if uuid::Uuid::parse_str(&job_id).is_err() {
        return Err("Invalid job identifier".into());
    }
    if request.image_name.trim().is_empty() {
        return Err("Select a source image first".into());
    }
    let profile = tauri::async_runtime::spawn_blocking(hardware::detect)
        .await
        .map_err(|error| error.to_string())?;
    let engine = catalog::engines(&profile)
        .into_iter()
        .find(|engine| engine.id == request.engine_id)
        .ok_or("Unknown engine profile")?;
    if engine.compatibility == "unsupported" {
        return Err(engine.reason);
    }
    let output_dir = app
        .path()
        .app_local_data_dir()
        .map_err(|e| e.to_string())?
        .join("jobs")
        .join(&job_id);
    let runtime: Box<dyn InferenceRuntime> = if request.engine_id == "triposr" {
        let source = harness
            .sources
            .lock()
            .map_err(|_| "Source registry unavailable")?
            .get(
                request
                    .source_id
                    .as_deref()
                    .ok_or("Import an image before generating")?,
            )
            .cloned()
            .ok_or("Unknown source image; import it again")?;
        let worker = paths::backend_file(&app, "worker.py")?;
        Box::new(python::PythonRuntime {
            root: paths::runtime_dir(&app)?,
            worker,
            source: source.path,
            source_sha256: source.asset.sha256,
            output_dir: output_dir.clone(),
        })
    } else if request.engine_id == "demo" {
        Box::new(MockRuntime)
    } else {
        return Err("This engine is not implemented. Select TripoSR or the explicit demo.".into());
    };
    let cancelled = Arc::new(AtomicBool::new(false));
    {
        let mut jobs = harness.jobs.lock().map_err(|_| "Job state unavailable")?;
        if jobs.contains_key(&job_id) {
            return Err("Job identifier already in use".into());
        }
        if !jobs.is_empty() {
            return Err(
                "A generation is already running. Cancel it before starting another.".into(),
            );
        }
        jobs.insert(job_id.clone(), cancelled.clone());
    }
    let _runtime_kind = runtime.kind();
    let context = JobContext {
        id: job_id.clone(),
        cancelled,
        progress: Arc::new(move |progress| {
            let _ = app.emit_to("main", "sculpt://generation-progress", progress);
        }),
    };
    let result = runtime.generate(request, context).await;
    if let Ok(mut jobs) = harness.jobs.lock() {
        jobs.remove(&job_id);
    }
    if result.as_ref().is_ok_and(|asset| !asset.simulated) {
        harness
            .outputs
            .lock()
            .map_err(|_| "Asset registry unavailable")?
            .insert(job_id, output_dir.join("mesh.glb"));
    }
    result
}

#[tauri::command]
pub fn cancel_generation(
    harness: State<'_, SculptInferenceHarness>,
    job_id: String,
) -> Result<(), String> {
    let jobs = harness.jobs.lock().map_err(|_| "Job state unavailable")?;
    if let Some(cancelled) = jobs.get(&job_id) {
        cancelled.store(true, Ordering::Relaxed);
    }
    Ok(())
}
