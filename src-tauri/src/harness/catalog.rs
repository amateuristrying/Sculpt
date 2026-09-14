use super::hardware::HardwareProfile;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineProfile {
    pub id: String,
    pub name: String,
    pub subtitle: String,
    pub description: String,
    pub model_size_gb: Option<f64>,
    pub runtime: String,
    pub compatibility: String,
    pub reason: String,
    pub implemented: bool,
}

pub fn engines(profile: &HardwareProfile) -> Vec<EngineProfile> {
    let apple_ready = profile.is_apple_silicon
        && profile
            .compute_backends
            .iter()
            .any(|backend| backend == "Metal")
        && profile.memory_gb >= 16.0;
    vec![
        EngineProfile {
            id: "triposr".into(), name: "TripoSR".into(), subtitle: "Local reconstruction · Experimental".into(),
            description: "Reconstruct a single object with geometry and vertex colors.".into(),
            model_size_gb: Some(1.8), runtime: "PyTorch / Metal".into(),
            compatibility: if apple_ready { "recommended" } else { "unsupported" }.into(),
            reason: if apple_ready {
                "Runs locally through an isolated Python worker. Best with one clearly visible object. Unseen surfaces are inferred; quality varies. One-time runtime setup is required."
            } else {
                "This first adapter targets Apple Silicon with Metal and at least 16 GB memory. Other hardware has not been validated."
            }.into(), implemented: true,
        },
        EngineProfile {
            id: "demo".into(), name: "Workspace Demo".into(), subtitle: "Procedural sample · No AI".into(),
            description: "Explore the viewport with a sample sculpture. Does not reconstruct your image.".into(), model_size_gb: None,
            runtime: "Local simulator".into(), compatibility: if apple_ready { "available" } else { "recommended" }.into(),
            reason: "An explicit demonstration of the workspace. Output is a procedural sculpture, not an AI reconstruction.".into(), implemented: true,
        },
        EngineProfile {
            id: "trellis-2".into(), name: "TRELLIS.2".into(), subtitle: "High quality".into(),
            description: "A demanding engine profile reserved for a future supported runtime.".into(), model_size_gb: None,
            runtime: "CUDA / PyTorch · planned".into(), compatibility: "unsupported".into(),
            reason: "No validated runtime or model adapter is included. Not recommended for this Mac; installation and generation are unavailable.".into(), implemented: false,
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m4() -> HardwareProfile {
        HardwareProfile {
            os: "macOS".into(),
            os_version: "15".into(),
            architecture: "arm64".into(),
            chip: "Apple M4".into(),
            gpu: "Apple M4".into(),
            memory_gb: 16.0,
            unified_memory: true,
            is_apple_silicon: true,
            compute_backends: vec!["Metal".into(), "CPU".into()],
            detection_source: "native".into(),
            storage_gb: Some(256.0),
        }
    }

    #[test]
    fn m4_recommends_implemented_adapter_and_keeps_future_engines_disabled() {
        let profiles = engines(&m4());
        assert_eq!(profiles[0].compatibility, "recommended");
        assert_eq!(profiles[0].id, "triposr");
        assert!(profiles[0].implemented);
        assert!(!profiles[2].implemented);
        assert_eq!(profiles[2].compatibility, "unsupported");
    }

    #[test]
    fn missing_metal_or_low_memory_does_not_recommend_apple_profile() {
        let mut hardware = m4();
        hardware.memory_gb = 8.0;
        assert_eq!(engines(&hardware)[0].compatibility, "unsupported");
        hardware.memory_gb = 16.0;
        hardware.compute_backends = vec!["CPU".into()];
        assert_eq!(engines(&hardware)[0].compatibility, "unsupported");
        assert_eq!(engines(&hardware)[1].compatibility, "recommended");
    }
}
