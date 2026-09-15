use super::{assets::read_valid_glb, runtime::*};
use serde::Serialize;
use serde_json::Value;
use std::{
    future::Future,
    path::{Path, PathBuf},
    pin::Pin,
    process::{Command, Stdio},
    sync::atomic::Ordering,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackendStatus {
    pub installed: bool,
    pub engine: String,
    pub runtime_path: String,
    pub message: String,
    pub mps_available: bool,
    pub recommended_quality: String,
    pub qualities: Value,
    pub state: String,
    pub download_cache_bytes: u64,
}

#[cfg(test)]
pub fn runtime_root() -> PathBuf {
    std::env::var_os("SCULPT_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .unwrap()
                .join(".sculpt-runtime")
        })
}

pub fn inspect(root: &Path) -> BackendStatus {
    let spec: Value = serde_json::from_str(include_str!("../../../backend/runtime-spec.json"))
        .expect("Bundled runtime specification");
    let manifest: Value = std::fs::read(root.join("ready.json"))
        .ok()
        .and_then(|v| serde_json::from_slice(&v).ok())
        .unwrap_or(Value::Null);
    let files = manifest["files"].as_object();
    let required = [
        "venv/bin/python",
        "TripoSR/tsr/system.py",
        "models/triposr/config.yaml",
        "models/triposr/model.ckpt",
        "models/background/u2net.onnx",
    ];
    let installed = manifest["version"] == spec["version"]
        && manifest["modelRevision"] == spec["modelRevision"]
        && manifest["sourceRevision"] == spec["sourceRevision"]
        && manifest["dependencyLockSha256"]
            == format!(
                "{:x}",
                <sha2::Sha256 as sha2::Digest>::digest(include_bytes!(
                    "../../../backend/requirements.lock"
                ))
            )
        && files.is_some_and(|files| {
            required.iter().all(|key| files.contains_key(*key))
                && files.iter().all(|(name, expected)| {
                    let path = Path::new(name);
                    if path.is_absolute()
                        || path
                            .components()
                            .any(|c| matches!(c, std::path::Component::ParentDir))
                    {
                        return false;
                    }
                    std::fs::metadata(root.join(path)).ok().is_some_and(|stat| {
                        let modified = stat
                            .modified()
                            .ok()
                            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                            .map(|d| d.as_nanos() as u64);
                        Some(stat.len()) == expected["size"].as_u64()
                            && modified == expected["modifiedNs"].as_u64()
                    })
                })
        });
    let repair = root.join("venv").exists();
    BackendStatus {
        download_cache_bytes: super::cache::download_bytes(root),
        installed,
        engine: "TripoSR".into(),
        runtime_path: root.to_string_lossy().into(),
        message: if installed {
            "Verified local model files are ready. Generation stays on this Mac."
        } else if repair {
            "Your local runtime needs verification or repair. Setup will reuse cached downloads."
        } else {
            "Set up your local engine once. Allow 5 GB on disk; internet is needed only for setup."
        }
        .into(),
        mps_available: installed && manifest["mpsAvailable"].as_bool().unwrap_or(false),
        recommended_quality: "balanced".into(),
        qualities: spec["qualities"].clone(),
        state: if installed {
            "ready"
        } else if repair {
            "repair"
        } else {
            "missing"
        }
        .into(),
    }
}

#[tauri::command]
pub async fn backend_status(app: tauri::AppHandle) -> Result<BackendStatus, String> {
    let root = super::paths::runtime_dir(&app)?;
    tauri::async_runtime::spawn_blocking(move || {
        let mut status = inspect(&root);
        let hardware = super::hardware::detect();
        status.recommended_quality = if hardware.memory_gb >= 24.0 {
            "high"
        } else {
            "balanced"
        }
        .into();
        status
    })
    .await
    .map_err(|e| e.to_string())
}

pub struct PythonRuntime {
    pub root: PathBuf,
    pub worker: PathBuf,
    pub source: PathBuf,
    pub source_sha256: String,
    pub output_dir: PathBuf,
}

impl InferenceRuntime for PythonRuntime {
    fn kind(&self) -> RuntimeKind {
        RuntimeKind::Pytorch
    }
    fn generate(
        &self,
        request: GenerationRequest,
        context: JobContext,
    ) -> Pin<Box<dyn Future<Output = Result<GeneratedAsset, String>> + Send>> {
        let root = self.root.clone();
        let worker = self.worker.clone();
        let source = self.source.clone();
        let output_dir = self.output_dir.clone();
        let source_sha256 = self.source_sha256.clone();
        Box::pin(async move {
            tauri::async_runtime::spawn_blocking(move || {
                run_worker(
                    &root,
                    &worker,
                    &source,
                    &source_sha256,
                    &output_dir,
                    request,
                    context,
                )
            })
            .await
            .map_err(|e| e.to_string())?
        })
    }
}

fn run_worker(
    root: &Path,
    worker: &Path,
    source: &Path,
    source_sha256: &str,
    output_dir: &Path,
    request: GenerationRequest,
    context: JobContext,
) -> Result<GeneratedAsset, String> {
    if context.cancelled.load(Ordering::Relaxed) {
        return Err("Generation cancelled".into());
    }
    if !inspect(root).installed {
        return Err(
            "The local runtime is missing or needs repair. Open runtime setup in Sculpt.".into(),
        );
    }
    if !worker.is_file() {
        return Err("The Sculpt Python worker is missing. Rebuild the desktop application.".into());
    }
    std::fs::create_dir_all(output_dir).map_err(|e| e.to_string())?;
    let output = output_dir.join("mesh.glb");
    let request_file = output_dir.join("request.json");
    // This adapter is offered as a validated Metal profile. CPU execution remains
    // an explicit developer option, never a silent fallback from the desktop UI.
    let body = serde_json::json!({"sourcePath":source, "outputPath":output, "quality":request.geometry, "background":request.background, "device":"mps"});
    std::fs::write(
        &request_file,
        serde_json::to_vec(&body).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    context.emit("analyzing", 1.0, "Starting the local inference worker");
    let log = std::fs::File::create(output_dir.join("worker.log")).map_err(|e| e.to_string())?;
    let mut command = Command::new(root.join("venv/bin/python"));
    command
        .arg(worker)
        .arg("--request")
        .arg(&request_file)
        .env("SCULPT_RUNTIME_DIR", root)
        .env("PYTHONUNBUFFERED", "1")
        .env("HF_HUB_OFFLINE", "1")
        .env("HF_HUB_DISABLE_TELEMETRY", "1")
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .env("OMP_NUM_THREADS", "4")
        .env("PYTORCH_ENABLE_MPS_FALLBACK", "1")
        .stderr(Stdio::from(log));
    let mut result_metrics: Option<Value> = None;
    let mut last_progress = 0.0;
    super::process::run(
        &mut command,
        &context.cancelled,
        Duration::from_secs(1200),
        |line| {
            let event: Value = serde_json::from_str(line)
                .map_err(|_| "Worker emitted an invalid protocol message")?;
            if event["protocol"] != 1 {
                return Err("Unsupported local worker protocol version".into());
            }
            match event["type"].as_str() {
                Some("progress") => {
                    let stage = event["stage"]
                        .as_str()
                        .ok_or("Worker progress has no stage")?;
                    if !["analyzing", "loading", "geometry", "surface", "preparing"]
                        .contains(&stage)
                    {
                        return Err("Unknown worker stage".into());
                    }
                    last_progress = event["progress"]
                        .as_f64()
                        .filter(|p| p.is_finite())
                        .ok_or("Invalid worker progress")?
                        .clamp(last_progress, 99.0);
                    context.emit(
                        stage,
                        last_progress,
                        event["message"].as_str().unwrap_or("Processing locally"),
                    );
                }
                Some("result") if event["metrics"].is_object() && result_metrics.is_none() => {
                    result_metrics = Some(event["metrics"].clone())
                }
                Some("error") => {
                    return Err(event["message"]
                        .as_str()
                        .unwrap_or("Local inference failed")
                        .chars()
                        .take(1500)
                        .collect())
                }
                _ => return Err("Unexpected local worker protocol message".into()),
            }
            Ok(())
        },
    )
    .map_err(|error| {
        if context.cancelled.load(Ordering::Relaxed) {
            context.emit("cancelled", last_progress, "Generation cancelled");
        }
        format!(
            "{error} Details: {}",
            output_dir.join("worker.log").display()
        )
    })?;
    let mut metrics = result_metrics.ok_or("The worker exited without returning an asset")?;
    let _ = read_valid_glb(&output)?;
    metrics["sourceSha256"] = source_sha256.into();
    std::fs::write(
        output_dir.join("metrics.json"),
        serde_json::to_vec_pretty(&metrics).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())?;
    context.emit("complete", 100.0, "Your reconstructed 3D asset is ready");
    Ok(GeneratedAsset {
        id: context.id,
        seed: 0,
        generated_at: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
            .to_string(),
        simulated: false,
        metrics: Some(metrics),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn missing_runtime_never_reports_ready() {
        assert!(!inspect(&std::env::temp_dir().join(uuid::Uuid::new_v4().to_string())).installed);
    }

    // Explicit opt-in: requires the installed weights and a local test image.
    // Exercises the same importer, subprocess supervisor and GLB validation as Tauri.
    #[test]
    #[ignore = "requires SCULPT_TEST_IMAGE and installed local model; uses the GPU"]
    fn real_image_runs_through_rust_supervisor() {
        use base64::Engine;
        use std::sync::{atomic::AtomicBool, Arc, Mutex};
        let image =
            std::env::var("SCULPT_TEST_IMAGE").expect("Set SCULPT_TEST_IMAGE to a JPEG fixture");
        let root = runtime_root();
        let id = uuid::Uuid::new_v4().to_string();
        let output = root.join("test-output").join(&id);
        let bytes = std::fs::read(image).unwrap();
        let record = super::super::assets::decode_source(
            "arbitrary-name.jpg".into(),
            &format!(
                "data:image/jpeg;base64,{}",
                base64::engine::general_purpose::STANDARD.encode(bytes)
            ),
            &output.join("sources"),
        )
        .unwrap();
        let events = Arc::new(Mutex::new(Vec::new()));
        let captured = events.clone();
        let context = JobContext {
            id: id.clone(),
            cancelled: Arc::new(AtomicBool::new(false)),
            progress: Arc::new(move |e| {
                captured.lock().unwrap().push(e);
            }),
        };
        let worker = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join("backend/worker.py");
        let request = GenerationRequest {
            engine_id: "triposr".into(),
            source_id: Some(record.asset.id),
            image_name: "arbitrary-name.jpg".into(),
            geometry: GeometryQuality::Draft,
            background: BackgroundMode::Auto,
        };
        let asset = run_worker(
            &root,
            &worker,
            &record.path,
            &record.asset.sha256,
            &output,
            request.clone(),
            context,
        )
        .unwrap();
        assert!(!asset.simulated);
        assert_eq!(
            asset.metrics.as_ref().unwrap()["sourceSha256"],
            record.asset.sha256
        );
        assert!(asset.metrics.as_ref().unwrap()["faces"].as_u64().unwrap() > 100);
        let events = events.lock().unwrap();
        assert_eq!(events.last().unwrap().stage, "complete");
        assert!(events
            .windows(2)
            .all(|pair| pair[0].progress <= pair[1].progress));
        println!("Real reconstruction: {}", asset.metrics.unwrap());

        let cancelled = Arc::new(AtomicBool::new(false));
        let flag = cancelled.clone();
        let context = JobContext {
            id: uuid::Uuid::new_v4().to_string(),
            cancelled,
            progress: Arc::new(move |e| {
                if e.stage == "geometry" {
                    flag.store(true, Ordering::Relaxed);
                }
            }),
        };
        let cancelled_output = output.join("cancelled");
        let result = run_worker(
            &root,
            &worker,
            &record.path,
            &record.asset.sha256,
            &cancelled_output,
            request,
            context,
        );
        assert!(result.unwrap_err().contains("cancelled"));
        assert!(!cancelled_output.join("mesh.glb").exists());
        println!("Cancellation terminated the real worker before mesh export.");
    }
}
