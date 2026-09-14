use serde::{Deserialize, Serialize};
use std::{
    future::Future,
    pin::Pin,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerationRequest {
    pub engine_id: String,
    pub image_name: String,
    /// Registered by native image import; the UI never supplies a worker path.
    #[serde(default)]
    pub source_id: Option<String>,
    pub geometry: GeometryQuality,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GeometryQuality {
    Draft,
    Balanced,
    High,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerationProgress {
    pub job_id: String,
    pub stage: String,
    pub progress: f64,
    pub message: String,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GeneratedAsset {
    pub id: String,
    pub seed: u32,
    /// Milliseconds since epoch internally; the facade normalizes this to ISO 8601.
    pub generated_at: String,
    pub simulated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metrics: Option<serde_json::Value>,
}

/// These runtimes are alternatives, not a requirement to convert every model to ONNX.
#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum RuntimeKind {
    Mock,
    Mlx,
    Pytorch,
    CudaPytorch,
    Onnx,
    Webgpu,
    Native,
}

#[allow(dead_code)]
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelArtifact {
    pub engine_id: String,
    pub version: String,
    pub state: ModelState,
    pub minimum_memory_gb: f64,
    pub compatible_runtimes: Vec<RuntimeKind>,
}

#[allow(dead_code)]
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ModelState {
    NotInstalled,
    Downloading,
    Ready,
    Loading,
    Loaded,
    Error,
}

pub struct JobContext {
    pub id: String,
    pub cancelled: Arc<AtomicBool>,
    pub progress: Arc<dyn Fn(GenerationProgress) + Send + Sync>,
}

impl JobContext {
    pub fn emit(&self, stage: &str, progress: f64, message: &str) {
        (self.progress)(GenerationProgress {
            job_id: self.id.clone(),
            stage: stage.into(),
            progress,
            message: message.into(),
        });
    }
}

/// Adapter boundary. Real implementations can supervise an MLX/PyTorch worker process;
/// the desktop application is not required to reimplement an engine in Rust.
pub trait InferenceRuntime: Send + Sync {
    fn kind(&self) -> RuntimeKind;
    fn generate(
        &self,
        request: GenerationRequest,
        context: JobContext,
    ) -> Pin<Box<dyn Future<Output = Result<GeneratedAsset, String>> + Send>>;
}

pub struct MockRuntime;

impl InferenceRuntime for MockRuntime {
    fn kind(&self) -> RuntimeKind {
        RuntimeKind::Mock
    }

    fn generate(
        &self,
        request: GenerationRequest,
        context: JobContext,
    ) -> Pin<Box<dyn Future<Output = Result<GeneratedAsset, String>> + Send>> {
        Box::pin(async move {
            let stages = [
                ("analyzing", "Analyzing image", 0.0, 18.0, 10),
                ("geometry", "Generating geometry", 18.0, 62.0, 22),
                ("surface", "Building surface", 62.0, 87.0, 14),
                ("preparing", "Preparing 3D asset", 87.0, 100.0, 9),
            ];
            let tick_ms = match request.geometry {
                GeometryQuality::Draft => 95,
                GeometryQuality::Balanced => 125,
                GeometryQuality::High => 155,
            };
            for (stage, message, start, end, ticks) in stages {
                for tick in 0..ticks {
                    let progress = start + (end - start) * f64::from(tick) / f64::from(ticks);
                    if context.cancelled.load(Ordering::Relaxed) {
                        context.emit("cancelled", progress, "Generation cancelled");
                        return Err("Generation cancelled".into());
                    }
                    context.emit(stage, progress, message);
                    tokio::time::sleep(Duration::from_millis(tick_ms)).await;
                }
            }
            if context.cancelled.load(Ordering::Relaxed) {
                context.emit("cancelled", 99.0, "Generation cancelled");
                return Err("Generation cancelled".into());
            }
            // Deterministic fixture identity, not image understanding or model inference.
            let seed = request
                .image_name
                .bytes()
                .fold(2_166_136_261_u32, |hash, byte| {
                    (hash ^ u32::from(byte)).wrapping_mul(16_777_619)
                });
            let result = GeneratedAsset {
                id: context.id.clone(),
                seed,
                generated_at: SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis()
                    .to_string(),
                simulated: true,
                metrics: None,
            };
            context.emit("complete", 100.0, "Preview asset ready");
            Ok(result)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[test]
    fn cancellation_stops_before_any_mock_work() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let collected = events.clone();
        let context = JobContext {
            id: "cancel-test".into(),
            cancelled: Arc::new(AtomicBool::new(true)),
            progress: Arc::new(move |event| collected.lock().unwrap().push(event)),
        };
        let request = GenerationRequest {
            engine_id: "demo".into(),
            image_name: "source.png".into(),
            source_id: None,
            geometry: GeometryQuality::Balanced,
        };
        let result = tauri::async_runtime::block_on(MockRuntime.generate(request, context));
        assert_eq!(result.unwrap_err(), "Generation cancelled");
        let events = events.lock().unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].stage, "cancelled");
        assert_eq!(events[0].job_id, "cancel-test");
    }
}
