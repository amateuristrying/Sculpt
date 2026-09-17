//! Trusted engine descriptors and deterministic hardware planning.
//!
//! A manifest describes an adapter compiled into Sculpt; it cannot introduce a
//! command, executable, or dynamically downloaded worker. Compatibility is not
//! inferred from an upstream model supporting a device: Sculpt must validate it.

use super::hardware::HardwareProfile;
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum RuntimeAdapter {
    #[serde(rename = "triposr")]
    TripoSr,
    Demo,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ComputeDevice {
    Mps,
    Cuda,
    Cpu,
}

impl ComputeDevice {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Mps => "mps",
            Self::Cuda => "cuda",
            Self::Cpu => "cpu",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EngineTarget {
    pub platform: String,
    pub architecture: String,
    pub compute_backend: String,
    pub device: ComputeDevice,
    /// Physical RAM admission floor; not a VRAM measurement or peak estimate.
    pub minimum_ram_gb: Option<f64>,
    pub requires_apple_silicon: bool,
    pub validated: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EngineManifest {
    pub schema_version: u32,
    pub id: String,
    pub name: String,
    /// Component license metadata, not a license audit of its dependencies.
    pub license_identifier: String,
    pub engine_revision: Option<String>,
    pub runtime_spec_path: Option<String>,
    pub adapter: Option<RuntimeAdapter>,
    pub implemented: bool,
    pub native_hardware_required: bool,
    pub targets: Vec<EngineTarget>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineAvailability {
    pub engine_id: String,
    pub available: bool,
    pub device: Option<ComputeDevice>,
    pub reason: String,
}

#[derive(Clone, Copy, Debug)]
pub struct EngineSelection {
    pub manifest: &'static EngineManifest,
    pub adapter: RuntimeAdapter,
    pub device: ComputeDevice,
}

impl EngineManifest {
    pub fn parse(json: &str) -> Result<Self, String> {
        let manifest: Self = serde_json::from_str(json).map_err(|e| e.to_string())?;
        manifest.validate()?;
        Ok(manifest)
    }

    fn validate(&self) -> Result<(), String> {
        if self.schema_version != 1 {
            return Err("Unsupported engine manifest schema".into());
        }
        if self.id.is_empty()
            || self.id.len() > 64
            || !self
                .id
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
            || self.name.trim().is_empty()
            || self.license_identifier.trim().is_empty()
        {
            return Err("Invalid engine identity or license metadata".into());
        }
        if self.implemented && (self.adapter.is_none() || self.targets.is_empty()) {
            return Err("An implemented engine requires a compiled adapter and targets".into());
        }
        // Runtime paths are descriptors, not user-provided filesystem paths. Add
        // new adapters here deliberately when their native integration exists.
        match self.adapter {
            Some(RuntimeAdapter::TripoSr) => {
                if self.id != "triposr"
                    || self.runtime_spec_path.as_deref() != Some("backend/runtime-spec.json")
                    || !self.engine_revision.as_ref().is_some_and(|revision| {
                        revision.len() == 40 && revision.bytes().all(|b| b.is_ascii_hexdigit())
                    })
                    || !self.native_hardware_required
                {
                    return Err(
                        "TripoSR requires its trusted runtime specification and pinned revision"
                            .into(),
                    );
                }
            }
            Some(RuntimeAdapter::Demo) => {
                if self.id != "demo"
                    || self.runtime_spec_path.is_some()
                    || self.engine_revision.is_some()
                {
                    return Err("The procedural demo cannot specify a model runtime".into());
                }
            }
            None if self.runtime_spec_path.is_some() => {
                return Err("An unimplemented engine cannot provide a runtime path".into());
            }
            None => {}
        }
        for target in &self.targets {
            if !["macos", "windows", "linux", "any"].contains(&target.platform.as_str())
                || !["arm64", "x86_64", "any"].contains(&target.architecture.as_str())
                || target
                    .minimum_ram_gb
                    .is_some_and(|gb| !gb.is_finite() || gb <= 0.0)
                || (target.requires_apple_silicon && target.platform != "macos")
                || match target.device {
                    ComputeDevice::Mps => {
                        target.platform != "macos" || target.compute_backend != "Metal"
                    }
                    ComputeDevice::Cuda => target.compute_backend != "CUDA",
                    ComputeDevice::Cpu => target.compute_backend != "CPU",
                }
            {
                return Err("Invalid engine hardware target".into());
            }
        }
        Ok(())
    }
}

/// Descriptors ship with the app and are reviewed alongside adapter code.
pub fn manifests() -> &'static [EngineManifest] {
    static MANIFESTS: OnceLock<Vec<EngineManifest>> = OnceLock::new();
    MANIFESTS.get_or_init(|| {
        let triposr = EngineManifest::parse(include_str!("../../../backend/engines/triposr.json"))
            .expect("Valid bundled TripoSR engine descriptor");
        let spec: serde_json::Value =
            serde_json::from_str(include_str!("../../../backend/runtime-spec.json"))
                .expect("Valid bundled TripoSR runtime specification");
        assert_eq!(
            triposr.engine_revision.as_deref(),
            spec["sourceRevision"].as_str(),
            "Engine descriptor and runtime specification must pin the same source"
        );
        let demo = EngineManifest::parse(
            r#"{
            "schemaVersion":1,"id":"demo","name":"Workspace Demo",
            "licenseIdentifier":"NOASSERTION","engineRevision":null,"runtimeSpecPath":null,
            "adapter":"demo","implemented":true,"nativeHardwareRequired":false,
            "targets":[{"platform":"any","architecture":"any","computeBackend":"CPU",
                "device":"cpu","minimumRamGb":null,"requiresAppleSilicon":false,"validated":true}]
        }"#,
        )
        .expect("Valid bundled demo descriptor");
        vec![triposr, demo]
    })
}

fn platform_matches(target: &EngineTarget, hardware: &HardwareProfile) -> bool {
    let platform = match hardware.os.to_ascii_lowercase().as_str() {
        "macos" | "darwin" => "macos",
        "windows" => "windows",
        "linux" => "linux",
        _ => "unknown",
    };
    let architecture = match hardware.architecture.as_str() {
        "aarch64" | "arm64" => "arm64",
        "x86_64" | "amd64" => "x86_64",
        _ => "unknown",
    };
    (target.platform == "any" || target.platform == platform)
        && (target.architecture == "any" || target.architecture == architecture)
        && (!target.requires_apple_silicon || hardware.is_apple_silicon)
}

fn plan_engine(
    hardware: &HardwareProfile,
    manifest: &EngineManifest,
) -> Result<ComputeDevice, String> {
    manifest.validate()?;
    if !manifest.implemented {
        return Err("No validated runtime adapter is included for this engine.".into());
    }
    if manifest.native_hardware_required && hardware.detection_source != "native" {
        return Err(
            "Open the desktop app to detect this machine and use local reconstruction.".into(),
        );
    }
    let matches_backend = |target: &&EngineTarget| {
        platform_matches(target, hardware)
            && hardware
                .compute_backends
                .iter()
                .any(|backend| backend == &target.compute_backend)
    };
    let validated: Vec<_> = manifest
        .targets
        .iter()
        .filter(|target| target.validated)
        .collect();
    for target in validated.iter().copied().filter(matches_backend) {
        if let Some(minimum) = target.minimum_ram_gb {
            if !hardware.memory_gb.is_finite() || hardware.memory_gb < minimum {
                return Err(format!("This validated runtime requires at least {minimum} GB of detected system memory."));
            }
        }
        return Ok(target.device);
    }
    if validated
        .iter()
        .any(|target| platform_matches(target, hardware))
    {
        return Err(
            "The compute backend required by this validated runtime was not detected.".into(),
        );
    }
    Err("This hardware has no validated Sculpt runtime for this engine. Installation and generation are unavailable.".into())
}

/// Pure planning: no downloads, process launches, filesystem reads or fallback.
pub fn plan(hardware: &HardwareProfile, manifests: &[EngineManifest]) -> Vec<EngineAvailability> {
    manifests
        .iter()
        .map(|manifest| match plan_engine(hardware, manifest) {
            Ok(device) => EngineAvailability {
                engine_id: manifest.id.clone(),
                available: true,
                device: Some(device),
                reason:
                    "A validated adapter is available; runtime installation is checked separately."
                        .into(),
            },
            Err(reason) => EngineAvailability {
                engine_id: manifest.id.clone(),
                available: false,
                device: None,
                reason,
            },
        })
        .collect()
}

/// Resolve only trusted bundled manifests using freshly detected native hardware.
pub fn resolve(engine_id: &str, hardware: &HardwareProfile) -> Result<EngineSelection, String> {
    let manifest = manifests()
        .iter()
        .find(|manifest| manifest.id == engine_id)
        .ok_or("Unknown or unimplemented engine profile")?;
    let device = plan_engine(hardware, manifest)?;
    let adapter = manifest
        .adapter
        .ok_or("No runtime adapter is implemented for this engine")?;
    Ok(EngineSelection {
        manifest,
        adapter,
        device,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m4() -> HardwareProfile {
        HardwareProfile {
            os: "macOS".into(),
            os_version: "15.2".into(),
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
    fn m4_resolves_the_validated_compiled_adapter() {
        let selected = resolve("triposr", &m4()).unwrap();
        assert_eq!(selected.adapter, RuntimeAdapter::TripoSr);
        assert_eq!(selected.device, ComputeDevice::Mps);
        assert_eq!(selected.device.as_str(), "mps");
        assert_eq!(selected.manifest.id, "triposr");
        assert!(plan(&m4(), manifests())[0].available);
    }

    #[test]
    fn intel_and_unknown_hardware_do_not_silently_fall_back_to_cpu() {
        let mut hardware = m4();
        hardware.architecture = "x86_64".into();
        hardware.is_apple_silicon = false;
        hardware.chip = "Intel Core i7".into();
        assert!(resolve("triposr", &hardware).is_err());
        hardware.os = "unknown".into();
        hardware.memory_gb = 0.0;
        hardware.compute_backends = vec!["CPU".into()];
        assert!(resolve("triposr", &hardware).is_err());
        assert_eq!(
            resolve("demo", &hardware).unwrap().adapter,
            RuntimeAdapter::Demo
        );
    }

    #[test]
    fn descriptors_do_not_enable_untested_cuda_targets() {
        let mut hardware = m4();
        hardware.os = "windows".into();
        hardware.architecture = "x86_64".into();
        hardware.is_apple_silicon = false;
        hardware.gpu = "NVIDIA RTX 5090".into();
        hardware.memory_gb = 64.0;
        hardware.compute_backends = vec!["CUDA".into(), "CPU".into()];
        assert!(resolve("triposr", &hardware)
            .unwrap_err()
            .contains("no validated"));
    }

    #[test]
    fn memory_backend_and_native_detection_are_all_required() {
        let mut hardware = m4();
        hardware.memory_gb = 8.0;
        assert!(resolve("triposr", &hardware).unwrap_err().contains("16 GB"));
        hardware.memory_gb = f64::NAN;
        assert!(resolve("triposr", &hardware).is_err());
        hardware.memory_gb = 16.0;
        hardware.compute_backends = vec!["CPU".into()];
        assert!(resolve("triposr", &hardware)
            .unwrap_err()
            .contains("compute backend"));
        hardware.compute_backends.push("Metal".into());
        hardware.detection_source = "browser".into();
        assert!(resolve("triposr", &hardware)
            .unwrap_err()
            .contains("desktop app"));
    }

    #[test]
    fn manifests_cannot_select_arbitrary_workers_or_specs() {
        let json = include_str!("../../../backend/engines/triposr.json");
        assert!(EngineManifest::parse(
            &json.replace("backend/runtime-spec.json", "../../worker.py")
        )
        .is_err());
        assert!(EngineManifest::parse(
            &json.replace("\"adapter\": \"triposr\"", "\"adapter\": \"shell\"")
        )
        .is_err());
        let mut value: serde_json::Value = serde_json::from_str(json).unwrap();
        value["workerCommand"] = serde_json::json!("arbitrary-command");
        assert!(EngineManifest::parse(&value.to_string()).is_err());
        assert!(resolve("../triposr", &m4()).is_err());
    }
}
