use super::engines::{evaluate_manifest, manifests, CompatibilityStatus, ResourcePreflight};
use super::hardware::HardwareProfile;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct EngineProfile {
    pub id: String,
    pub name: String,
    pub subtitle: String,
    pub description: String,
    pub model_size_gb: Option<f64>,
    pub runtime: String,
    pub compatibility: String,
    pub compatibility_status: CompatibilityStatus,
    pub reason: String,
    pub recommendation_reason: Option<String>,
    pub requires_confirmation: bool,
    pub confirmation_reason: Option<String>,
    #[serde(default)]
    pub requires_cpu_confirmation: bool,
    pub resource_preflight: ResourcePreflight,
    pub implemented: bool,
}

fn engine_metadata(id: &str) -> (&'static str, &'static str, &'static str) {
    match id {
        "triposr" => (
            "Local reconstruction · Experimental",
            "Reconstruct a single object with geometry and vertex colors.",
            "PyTorch / Metal",
        ),
        "demo" => (
            "Procedural sample · No AI",
            "Explore the viewport with a sample sculpture. Does not reconstruct your image.",
            "Local simulator",
        ),
        "trellis-2" => (
            "High quality",
            "A demanding engine profile reserved for a future supported runtime.",
            "CUDA / PyTorch · planned",
        ),
        _ => (
            "Community adapter",
            "Generate 3D assets locally.",
            "Native worker",
        ),
    }
}

pub fn engines(profile: &HardwareProfile) -> Vec<EngineProfile> {
    let all_manifests = manifests();
    let evaluations: Vec<super::engines::EngineEvaluation> = all_manifests
        .iter()
        .map(|manifest| evaluate_manifest(profile, manifest))
        .collect();

    let triposr_supported = evaluations
        .iter()
        .find(|e| e.engine_id == "triposr")
        .map(|e| e.compatibility_status == CompatibilityStatus::Supported)
        .unwrap_or(false);

    evaluations
        .into_iter()
        .zip(all_manifests.iter())
        .map(|(eval, manifest)| {
            let (subtitle, default_desc, default_runtime) = engine_metadata(&manifest.id);
            let mut compatibility = eval.compatibility;
            let mut recommendation_reason = eval.recommendation_reason;

            if manifest.id == "demo" {
                if triposr_supported {
                    compatibility = "available".into();
                    recommendation_reason = Some("Procedural sample available for exploration.".into());
                } else {
                    compatibility = "recommended".into();
                    recommendation_reason =
                        Some("Recommended zero-dependency fallback workspace demo.".into());
                }
            } else if manifest.id == "triposr" {
                if eval.compatibility_status == CompatibilityStatus::Supported {
                    compatibility = "recommended".into();
                } else if eval.compatibility_status == CompatibilityStatus::Experimental
                    || eval.compatibility_status == CompatibilityStatus::CompatibleUnverified
                {
                    compatibility = "available".into();
                } else {
                    compatibility = "unsupported".into();
                }
            }

            let reason = if manifest.id == "demo" {
                "An explicit demonstration of the workspace. Output is a procedural sculpture, not an AI reconstruction.".into()
            } else if manifest.id == "triposr" && eval.compatibility_status == CompatibilityStatus::Supported {
                "Runs locally through an isolated Python worker. Best with one clearly visible object. Unseen surfaces are inferred; quality varies. One-time runtime setup is required.".into()
            } else {
                eval.reason
            };

            let model_size_gb = manifest
                .resource_thresholds
                .as_ref()
                .and_then(|r| r.disk_download_bytes)
                .map(|b| (b as f64) / 1_000_000_000.0);

            let description = manifest.description.clone().unwrap_or_else(|| default_desc.into());

            EngineProfile {
                id: manifest.id.clone(),
                name: manifest.name.clone(),
                subtitle: subtitle.into(),
                description,
                model_size_gb,
                runtime: default_runtime.into(),
                compatibility,
                compatibility_status: eval.compatibility_status,
                reason,
                recommendation_reason,
                requires_confirmation: eval.requires_confirmation,
                confirmation_reason: eval.confirmation_reason,
                requires_cpu_confirmation: eval.requires_cpu_confirmation,
                resource_preflight: eval.resource_preflight,
                implemented: manifest.implemented,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::harness::engines::{
        ComputeDevice, EngineCapabilities, EngineManifest, EngineTarget, InputModality,
        MaterialCapabilities, ResourceThresholds, TargetValidationStatus,
    };

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
            npu: Some("Apple Neural Engine".into()),
        }
    }

    #[test]
    fn m4_recommends_implemented_adapter_and_keeps_future_engines_disabled() {
        let profiles = engines(&m4());
        assert_eq!(profiles[0].compatibility, "recommended");
        assert_eq!(
            profiles[0].compatibility_status,
            CompatibilityStatus::Supported
        );
        assert_eq!(profiles[0].id, "triposr");
        assert!(profiles[0].implemented);
        assert!(!profiles[0].requires_confirmation);
        assert!(!profiles[0].requires_cpu_confirmation);
        assert!(profiles[0].resource_preflight.passed);
        assert!(profiles[0].resource_preflight.ram_passed);
        assert!(profiles[0].resource_preflight.storage_passed);

        assert_eq!(profiles[1].id, "demo");
        assert_eq!(profiles[1].compatibility, "available");
        assert_eq!(
            profiles[1].compatibility_status,
            CompatibilityStatus::Supported
        );

        assert_eq!(profiles[2].id, "trellis-2");
        assert!(!profiles[2].implemented);
        assert_eq!(profiles[2].compatibility, "unsupported");
        assert_eq!(
            profiles[2].compatibility_status,
            CompatibilityStatus::Unsupported
        );
    }

    #[test]
    fn missing_metal_or_low_memory_does_not_recommend_apple_profile() {
        let mut hardware = m4();
        hardware.memory_gb = 8.0;
        let low_mem_profiles = engines(&hardware);
        assert_eq!(low_mem_profiles[0].compatibility, "unsupported");
        assert_eq!(
            low_mem_profiles[0].compatibility_status,
            CompatibilityStatus::InsufficientResources
        );
        assert!(!low_mem_profiles[0].resource_preflight.passed);
        assert!(!low_mem_profiles[0].resource_preflight.ram_passed);

        hardware.memory_gb = 16.0;
        hardware.compute_backends = vec!["CPU".into()];
        let no_metal_profiles = engines(&hardware);
        assert_eq!(no_metal_profiles[0].compatibility, "unsupported");
        assert_eq!(
            no_metal_profiles[0].compatibility_status,
            CompatibilityStatus::Unsupported
        );
        assert_eq!(no_metal_profiles[1].compatibility, "recommended");
        assert_eq!(
            no_metal_profiles[1].compatibility_status,
            CompatibilityStatus::Supported
        );
    }

    #[test]
    fn catalog_and_native_resolver_agree_on_browser_and_unsupported_os() {
        let mut hardware = m4();
        hardware.detection_source = "browser".into();
        assert_eq!(engines(&hardware)[0].compatibility, "unsupported");
        assert_eq!(
            engines(&hardware)[0].compatibility_status,
            CompatibilityStatus::Unsupported
        );
        assert!(engines(&hardware)[0].reason.contains("desktop app"));
        hardware.detection_source = "native".into();
        hardware.os = "linux".into();
        assert_eq!(engines(&hardware)[0].compatibility, "unsupported");
        assert_eq!(
            engines(&hardware)[0].compatibility_status,
            CompatibilityStatus::Unsupported
        );
        assert!(super::super::engines::resolve("triposr", &hardware).is_err());
    }

    #[test]
    fn hardware_detection_does_not_infer_engine_support() {
        // High-end Windows x64 with NVIDIA RTX 4090:
        // CUDA is detected and hardware is powerful, but Sculpt has not validated TripoSR on Windows.
        // It must be marked experimental and available, NEVER supported or recommended.
        let win_cuda = HardwareProfile {
            os: "windows".into(),
            os_version: "11".into(),
            architecture: "x86_64".into(),
            chip: "AMD Ryzen 9 9950X".into(),
            gpu: "NVIDIA GeForce RTX 4090".into(),
            memory_gb: 64.0,
            unified_memory: false,
            is_apple_silicon: false,
            compute_backends: vec!["CUDA".into(), "DirectML".into(), "CPU".into()],
            detection_source: "native".into(),
            storage_gb: Some(1024.0),
            npu: None,
        };
        let profiles = engines(&win_cuda);
        assert_eq!(profiles[0].id, "triposr");
        assert_eq!(
            profiles[0].compatibility_status,
            CompatibilityStatus::Experimental
        );
        assert_eq!(profiles[0].compatibility, "available");
        // Demo is recommended because no AI engine is validated/supported yet on Windows
        assert_eq!(profiles[1].id, "demo");
        assert_eq!(profiles[1].compatibility, "recommended");

        // Windows ARM64 with Snapdragon X Elite:
        // No validated target exists for Snapdragon ARM64 -> unsupported
        let win_arm = HardwareProfile {
            os: "windows".into(),
            os_version: "11".into(),
            architecture: "arm64".into(),
            chip: "Snapdragon X Elite".into(),
            gpu: "Qualcomm Adreno".into(),
            memory_gb: 32.0,
            unified_memory: true,
            is_apple_silicon: false,
            compute_backends: vec!["Vulkan".into(), "DirectML".into(), "CPU".into()],
            detection_source: "native".into(),
            storage_gb: Some(512.0),
            npu: Some("Qualcomm Hexagon NPU".into()),
        };
        let arm_profiles = engines(&win_arm);
        assert_eq!(arm_profiles[0].id, "triposr");
        assert_eq!(
            arm_profiles[0].compatibility_status,
            CompatibilityStatus::Unsupported
        );
        assert_eq!(arm_profiles[0].compatibility, "unsupported");
    }

    #[test]
    fn cpu_fallback_requires_explicit_confirmation_and_is_never_silent() {
        // 1. TripoSR CPU target is explicitly unsupported in manifest.
        // Even when hardware has massive CPU resources, TripoSR must never silently downgrade to CPU.
        let cpu_workstation = HardwareProfile {
            os: "macos".into(),
            os_version: "15".into(),
            architecture: "arm64".into(),
            chip: "Apple M4 Max".into(),
            gpu: "Unknown".into(),
            memory_gb: 64.0,
            unified_memory: true,
            is_apple_silicon: true,
            compute_backends: vec!["CPU".into()],
            detection_source: "native".into(),
            storage_gb: Some(1000.0),
            npu: Some("Apple Neural Engine".into()),
        };
        let profiles = engines(&cpu_workstation);
        assert_eq!(profiles[0].id, "triposr");
        assert_eq!(
            profiles[0].compatibility_status,
            CompatibilityStatus::Unsupported
        );
        assert_eq!(profiles[0].compatibility, "unsupported");

        // 2. An AI engine manifest that DOES explicitly support CPU execution:
        // When running on CPU, it requires confirmation and sets the warning.
        let cpu_supported_manifest = EngineManifest {
            schema_version: 1,
            id: "cpu-reconstruction".into(),
            name: "CPU Reconstruction Engine".into(),
            description: Some("Model capable of running on CPU".into()),
            publisher: Some("Test".into()),
            upstream_url: None,
            license_identifier: "MIT".into(),
            engine_revision: None,
            runtime_spec_path: None,
            adapter: Some(crate::harness::engines::RuntimeAdapter::TripoSr),
            implemented: true,
            native_hardware_required: true,
            supported_inputs: vec![InputModality::SingleImage],
            material_capabilities: MaterialCapabilities::vertex_color_only(),
            resource_thresholds: Some(ResourceThresholds {
                ram_min_gb: Some(16.0),
                ram_recommended_gb: Some(32.0),
                vram_min_gb: None,
                vram_recommended_gb: None,
                disk_download_bytes: Some(1000000000),
                installed_footprint_bytes: Some(2000000000),
            }),
            capabilities: EngineCapabilities::default(),
            targets: vec![
                EngineTarget {
                    platform: "macos".into(),
                    architecture: "arm64".into(),
                    compute_backend: "Metal".into(),
                    device: ComputeDevice::Mps,
                    minimum_ram_gb: Some(16.0),
                    recommended_ram_gb: Some(32.0),
                    minimum_vram_gb: None,
                    requires_apple_silicon: true,
                    validated: true,
                    validation_status: Some(TargetValidationStatus::Validated),
                },
                EngineTarget {
                    platform: "any".into(),
                    architecture: "any".into(),
                    compute_backend: "CPU".into(),
                    device: ComputeDevice::Cpu,
                    minimum_ram_gb: Some(16.0),
                    recommended_ram_gb: Some(32.0),
                    minimum_vram_gb: None,
                    requires_apple_silicon: false,
                    validated: true,
                    validation_status: Some(TargetValidationStatus::Validated),
                },
            ],
            known_limits: vec![],
            notes: None,
        };

        // When Metal is present on M4, GPU target is preferred over CPU; no confirmation required
        let gpu_eval = evaluate_manifest(&m4(), &cpu_supported_manifest);
        assert_eq!(
            gpu_eval.compatibility_status,
            CompatibilityStatus::Supported
        );
        assert!(!gpu_eval.requires_confirmation);
        assert!(!gpu_eval.requires_cpu_confirmation);
        assert!(gpu_eval.confirmation_reason.is_none());

        // When only CPU is available, candidate selects CPU:
        // Confirmation is mandatory and specifies performance/thermal impact.
        let cpu_eval = evaluate_manifest(&cpu_workstation, &cpu_supported_manifest);
        assert_eq!(
            cpu_eval.compatibility_status,
            CompatibilityStatus::Supported
        );
        assert!(cpu_eval.requires_confirmation);
        assert!(cpu_eval.requires_cpu_confirmation);
        assert!(cpu_eval
            .confirmation_reason
            .as_ref()
            .unwrap()
            .contains("CPU execution is significantly slower"));

        // If CPU workstation lacks minimum RAM (e.g. 8 GB vs 16 GB required), CPU fallback is gated out
        let mut low_ram_cpu = cpu_workstation.clone();
        low_ram_cpu.memory_gb = 8.0;
        let low_ram_cpu_eval = evaluate_manifest(&low_ram_cpu, &cpu_supported_manifest);
        assert_eq!(
            low_ram_cpu_eval.compatibility_status,
            CompatibilityStatus::InsufficientResources
        );
        assert_eq!(low_ram_cpu_eval.compatibility, "unsupported");

        // 3. Demo engine (procedural sample knot, no AI) does NOT require CPU confirmation
        let demo_eval = evaluate_manifest(&cpu_workstation, &manifests()[1]);
        assert_eq!(demo_eval.engine_id, "demo");
        assert!(!demo_eval.requires_confirmation);
        assert!(!demo_eval.requires_cpu_confirmation);
    }

    #[test]
    fn compatible_unverified_status_evaluation() {
        let unverified_manifest = EngineManifest {
            schema_version: 1,
            id: "experimental-onnx".into(),
            name: "ONNX DirectML Engine".into(),
            description: Some("Unverified runtime target".into()),
            publisher: Some("Community".into()),
            upstream_url: None,
            license_identifier: "Apache-2.0".into(),
            engine_revision: None,
            runtime_spec_path: None,
            adapter: Some(crate::harness::engines::RuntimeAdapter::TripoSr),
            implemented: true,
            native_hardware_required: true,
            supported_inputs: vec![InputModality::SingleImage],
            material_capabilities: MaterialCapabilities::vertex_color_only(),
            resource_thresholds: None,
            capabilities: EngineCapabilities::default(),
            targets: vec![EngineTarget {
                platform: "windows".into(),
                architecture: "x86_64".into(),
                compute_backend: "DirectML".into(),
                device: ComputeDevice::Directml,
                minimum_ram_gb: Some(8.0),
                recommended_ram_gb: None,
                minimum_vram_gb: None,
                requires_apple_silicon: false,
                validated: false,
                validation_status: Some(TargetValidationStatus::CompatibleUnverified),
            }],
            known_limits: vec![],
            notes: None,
        };

        let win_directml = HardwareProfile {
            os: "windows".into(),
            os_version: "11".into(),
            architecture: "x86_64".into(),
            chip: "Intel Core i7".into(),
            gpu: "Intel Arc A770".into(),
            memory_gb: 16.0,
            unified_memory: false,
            is_apple_silicon: false,
            compute_backends: vec!["DirectML".into(), "CPU".into()],
            detection_source: "native".into(),
            storage_gb: Some(256.0),
            npu: None,
        };

        let eval = evaluate_manifest(&win_directml, &unverified_manifest);
        assert_eq!(
            eval.compatibility_status,
            CompatibilityStatus::CompatibleUnverified
        );
        assert_eq!(eval.compatibility, "available");
        assert!(eval.reason.contains("has not been validated end-to-end"));
    }

    #[test]
    fn serialization_roundtrip_and_camel_case_schema() {
        let profiles = engines(&m4());
        let json = serde_json::to_string(&profiles[0]).unwrap();
        assert!(json.contains("\"compatibilityStatus\":\"supported\""));
        assert!(json.contains("\"requiresConfirmation\":false"));
        assert!(json.contains("\"requiresCpuConfirmation\":false"));
        assert!(json.contains("\"resourcePreflight\":{"));
        assert!(json.contains("\"ramPassed\":true"));

        let roundtrip: EngineProfile = serde_json::from_str(&json).unwrap();
        assert_eq!(roundtrip, profiles[0]);
    }
}
