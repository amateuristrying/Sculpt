use super::{assets::read_valid_glb, runtime::*};
use serde::Serialize;
use serde_json::Value;
use std::{future::Future, io::{BufRead, BufReader}, path::{Path, PathBuf}, pin::Pin, process::{Command, Stdio}, sync::{atomic::Ordering, mpsc}, time::{Duration, Instant, SystemTime, UNIX_EPOCH}};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackendStatus {
    pub installed: bool,
    pub engine: String,
    pub runtime_path: String,
    pub message: String,
    pub mps_available: bool,
}

pub fn runtime_root() -> PathBuf {
    std::env::var_os("SCULPT_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().join(".sculpt-runtime"))
}

pub fn inspect(root: &Path) -> BackendStatus {
    let manifest: Value = std::fs::read(root.join("ready.json")).ok().and_then(|v| serde_json::from_slice(&v).ok()).unwrap_or(Value::Null);
    let installed = ["venv/bin/python", "TripoSR/tsr/system.py", "models/triposr/config.yaml", "models/triposr/model.ckpt", "models/background/u2net.onnx"].iter().all(|file| root.join(file).is_file())
        && manifest["modelRevision"] == "5b521936b01fbe1890f6f9baed0254ab6351c04a"
        && manifest["sourceRevision"] == "107cefdc244c39106fa830359024f6a2f1c78871";
    BackendStatus { installed, engine: "TripoSR".into(), runtime_path: root.to_string_lossy().into(), message: if installed { "Local TripoSR runtime and model files are installed." } else { "Install the local runtime with npm run backend:setup, then refresh this screen." }.into(), mps_available: manifest["mpsAvailable"].as_bool().unwrap_or(false) }
}

#[tauri::command]
pub fn backend_status() -> BackendStatus { inspect(&runtime_root()) }

pub struct PythonRuntime {
    pub root: PathBuf,
    pub worker: PathBuf,
    pub source: PathBuf,
    pub source_sha256: String,
    pub output_dir: PathBuf,
}

impl InferenceRuntime for PythonRuntime {
    fn kind(&self) -> RuntimeKind { RuntimeKind::Pytorch }
    fn generate(&self, request: GenerationRequest, context: JobContext) -> Pin<Box<dyn Future<Output = Result<GeneratedAsset, String>> + Send>> {
        let root = self.root.clone(); let worker = self.worker.clone(); let source = self.source.clone(); let output_dir = self.output_dir.clone(); let source_sha256 = self.source_sha256.clone();
        Box::pin(async move { tauri::async_runtime::spawn_blocking(move || run_worker(&root, &worker, &source, &source_sha256, &output_dir, request, context)).await.map_err(|e| e.to_string())? })
    }
}

fn run_worker(root: &Path, worker: &Path, source: &Path, source_sha256: &str, output_dir: &Path, request: GenerationRequest, context: JobContext) -> Result<GeneratedAsset, String> {
    if context.cancelled.load(Ordering::Relaxed) { return Err("Generation cancelled".into()); }
    if !inspect(root).installed { return Err("The TripoSR runtime is not installed. Run npm run backend:setup.".into()); }
    if !worker.is_file() { return Err("The Sculpt Python worker is missing. Rebuild the desktop application.".into()); }
    std::fs::create_dir_all(output_dir).map_err(|e| e.to_string())?;
    let output = output_dir.join("mesh.glb");
    let request_file = output_dir.join("request.json");
    let body = serde_json::json!({"sourcePath":source, "outputPath":output, "quality":request.geometry, "device":"auto"});
    std::fs::write(&request_file, serde_json::to_vec(&body).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    context.emit("analyzing", 1.0, "Starting the local inference worker");
    let log = std::fs::File::create(output_dir.join("worker.log")).map_err(|e| e.to_string())?;
    let mut child = Command::new(root.join("venv/bin/python"))
        .arg(worker).arg("--request").arg(&request_file)
        .env("SCULPT_RUNTIME_DIR", root).env("PYTHONUNBUFFERED", "1")
        .env("HF_HUB_OFFLINE", "1").env("HF_HUB_DISABLE_TELEMETRY", "1")
        .env("OMP_NUM_THREADS", "4").env("PYTORCH_ENABLE_MPS_FALLBACK", "1")
        .stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::from(log)).spawn()
        .map_err(|e| format!("Could not start local Python runtime: {e}"))?;
    let stdout = child.stdout.take().ok_or("Worker output is unavailable")?;
    let (tx, rx) = mpsc::channel();
    let reader = std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            if tx.send(line).is_err() { break; }
        }
    });
    let started = Instant::now();
    let mut result_metrics: Option<Value> = None;
    let mut failure: Option<String> = None;
    let mut last_progress = 0.0;
    loop {
        if context.cancelled.load(Ordering::Relaxed) || started.elapsed() > Duration::from_secs(1200) {
            let _ = child.kill(); let _ = child.wait(); let _ = reader.join();
            return Err(if context.cancelled.load(Ordering::Relaxed) { context.emit("cancelled", last_progress, "Generation cancelled"); "Generation cancelled" } else { "Generation exceeded the 20-minute limit. Try Draft quality." }.into());
        }
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(Ok(line)) => {
                if line.len() > 65536 { failure = Some("Worker emitted an oversized protocol message".into()); break; }
                let event: Value = match serde_json::from_str(&line) { Ok(v) => v, Err(_) => { failure = Some("Worker emitted an invalid protocol message".into()); break; } };
                if event["protocol"] != 1 { failure = Some("Unsupported local worker protocol version".into()); break; }
                match event["type"].as_str() {
                    Some("progress") => {
                        let progress = event["progress"].as_f64().unwrap_or(last_progress).clamp(last_progress, 99.0);
                        last_progress = progress;
                        context.emit(event["stage"].as_str().unwrap_or("geometry"), progress, event["message"].as_str().unwrap_or("Processing locally"));
                    },
                    Some("result") => result_metrics = Some(event["metrics"].clone()),
                    Some("error") => failure = Some(event["message"].as_str().unwrap_or("Local inference failed").chars().take(1500).collect()),
                    _ => { failure = Some("Unexpected local worker protocol message".into()); break; }
                }
            },
            Ok(Err(e)) => { failure = Some(format!("Could not read local worker output: {e}")); break; },
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    if failure.is_some() { let _ = child.kill(); }
    let status = child.wait().map_err(|e| e.to_string())?; let _ = reader.join();
    if let Some(error) = failure { return Err(error); }
    if context.cancelled.load(Ordering::Relaxed) { return Err("Generation cancelled".into()); }
    if !status.success() { return Err(format!("Local inference stopped ({status}). Details are in {}", output_dir.join("worker.log").display())); }
    let mut metrics = result_metrics.ok_or("The worker exited without returning an asset")?;
    let _ = read_valid_glb(&output)?;
    metrics["sourceSha256"] = source_sha256.into();
    context.emit("complete", 100.0, "Your reconstructed 3D asset is ready");
    Ok(GeneratedAsset { id: context.id, seed: 0, generated_at: SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis().to_string(), simulated: false, metrics: Some(metrics) })
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
        use std::sync::{Arc, Mutex, atomic::AtomicBool};
        let image = std::env::var("SCULPT_TEST_IMAGE").expect("Set SCULPT_TEST_IMAGE to a JPEG fixture");
        let root = runtime_root();
        let id = uuid::Uuid::new_v4().to_string();
        let output = root.join("test-output").join(&id);
        let bytes = std::fs::read(image).unwrap();
        let record = super::super::assets::decode_source("arbitrary-name.jpg".into(), &format!("data:image/jpeg;base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes)), &output.join("sources")).unwrap();
        let events = Arc::new(Mutex::new(Vec::new()));
        let captured = events.clone();
        let context = JobContext { id: id.clone(), cancelled: Arc::new(AtomicBool::new(false)), progress: Arc::new(move |e| { captured.lock().unwrap().push(e); }) };
        let worker = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap().join("backend/worker.py");
        let request = GenerationRequest { engine_id: "triposr".into(), source_id: Some(record.asset.id), image_name: "arbitrary-name.jpg".into(), geometry: GeometryQuality::Draft };
        let asset = run_worker(&root, &worker, &record.path, &record.asset.sha256, &output, request.clone(), context).unwrap();
        assert!(!asset.simulated);
        assert_eq!(asset.metrics.as_ref().unwrap()["sourceSha256"], record.asset.sha256);
        assert!(asset.metrics.as_ref().unwrap()["faces"].as_u64().unwrap() > 100);
        let events = events.lock().unwrap();
        assert_eq!(events.last().unwrap().stage, "complete");
        assert!(events.windows(2).all(|pair| pair[0].progress <= pair[1].progress));
        println!("Real reconstruction: {}", asset.metrics.unwrap());

        let cancelled = Arc::new(AtomicBool::new(false));
        let flag = cancelled.clone();
        let context = JobContext { id: uuid::Uuid::new_v4().to_string(), cancelled, progress: Arc::new(move |e| { if e.stage == "geometry" { flag.store(true, Ordering::Relaxed); } }) };
        let cancelled_output = output.join("cancelled");
        let result = run_worker(&root, &worker, &record.path, &record.asset.sha256, &cancelled_output, request, context);
        assert!(result.unwrap_err().contains("cancelled"));
        assert!(!cancelled_output.join("mesh.glb").exists());
        println!("Cancellation terminated the real worker before mesh export.");
    }
}
