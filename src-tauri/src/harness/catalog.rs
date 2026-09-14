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
            id: "sf3d-apple".into(), name: "SF3D".into(), subtitle: "Apple Optimized · Planned".into(),
            description: "Target balance of quality and local performance. Apple adapter requires validation.".into(),
            model_size_gb: Some(4.0), runtime: "MLX / Metal · planned".into(),
            compatibility: if apple_ready { "recommended" } else { "unsupported" }.into(),
            reason: if apple_ready {
                "Hardware matches the prototype target. SF3D is not installed or integrated; this selection runs simulated generation. The 4 GB size is a planning estimate."
            } else {
                "Prototype target requires Apple Silicon, detected Metal, and 16 GB memory. No validated adapter is included."
            }.into(), implemented: false,
        },
        EngineProfile {
            id: "lightweight".into(), name: "Lightweight Engine".into(), subtitle: "Small footprint · Planned".into(),
            description: "A future low-memory option for quick shape studies.".into(), model_size_gb: None,
            runtime: "Portable runtime · planned".into(), compatibility: if apple_ready { "available" } else { "recommended" }.into(),
            reason: "Portable engine selection is still under evaluation. This prototype runs simulated generation without downloading models.".into(), implemented: false,
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
    fn m4_recommendation_never_claims_real_inference() {
        let profiles = engines(&m4());
        assert_eq!(profiles[0].compatibility, "recommended");
        assert!(profiles.iter().all(|profile| !profile.implemented));
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
