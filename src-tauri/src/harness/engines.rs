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
    Vulkan,
    Directml,
    Cpu,
}

impl ComputeDevice {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Mps => "mps",
            Self::Cuda => "cuda",
            Self::Vulkan => "vulkan",
            Self::Directml => "directml",
            Self::Cpu => "cpu",
        }
    }
}

/// Supported input modalities for generation.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum InputModality {
    SingleImage,
    MultiImage,
    TextPrompt,
}

fn default_supported_inputs() -> Vec<InputModality> {
    vec![InputModality::SingleImage]
}

/// Explicit material channels produced by an engine.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MaterialChannel {
    VertexColor,
    DiffuseColor,
    Normal,
    Roughness,
    Metallic,
    AmbientOcclusion,
    Emissive,
}

/// Material capabilities. Enforces that full PBR is never claimed when channels are missing.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct MaterialCapabilities {
    pub channels: Vec<MaterialChannel>,
    #[serde(default)]
    pub claims_pbr: bool,
}

impl MaterialCapabilities {
    pub fn vertex_color_only() -> Self {
        Self {
            channels: vec![MaterialChannel::VertexColor],
            claims_pbr: false,
        }
    }

    /// Full PBR requires at minimum a color channel (diffuse or vertex color), normal, roughness,
    /// and metallic.
    pub fn has_full_pbr(&self) -> bool {
        let has_color = self.channels.contains(&MaterialChannel::DiffuseColor)
            || self.channels.contains(&MaterialChannel::VertexColor);
        let has_normal = self.channels.contains(&MaterialChannel::Normal);
        let has_roughness = self.channels.contains(&MaterialChannel::Roughness);
        let has_metallic = self.channels.contains(&MaterialChannel::Metallic);
        has_color && has_normal && has_roughness && has_metallic
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.channels.is_empty() {
            return Err("Material capabilities must specify at least one output channel".into());
        }
        if self.claims_pbr && !self.has_full_pbr() {
            return Err(
                "Full PBR cannot be claimed when essential material channels (diffuse/albedo, normal, roughness, metallic) are missing"
                    .into(),
            );
        }
        Ok(())
    }
}

impl Default for MaterialCapabilities {
    fn default() -> Self {
        Self::vertex_color_only()
    }
}

/// Hardware resource thresholds for memory and disk usage.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ResourceThresholds {
    #[serde(default)]
    pub ram_min_gb: Option<f64>,
    #[serde(default)]
    pub ram_recommended_gb: Option<f64>,
    #[serde(default)]
    pub vram_min_gb: Option<f64>,
    #[serde(default)]
    pub vram_recommended_gb: Option<f64>,
    #[serde(default)]
    pub disk_download_bytes: Option<u64>,
    #[serde(default)]
    pub installed_footprint_bytes: Option<u64>,
}

impl ResourceThresholds {
    pub fn validate(&self) -> Result<(), String> {
        if let Some(ram) = self.ram_min_gb {
            if !ram.is_finite() || ram <= 0.0 {
                return Err("Minimum RAM must be a positive finite number".into());
            }
        }
        if let Some(rec_ram) = self.ram_recommended_gb {
            if !rec_ram.is_finite() || rec_ram <= 0.0 {
                return Err("Recommended RAM must be a positive finite number".into());
            }
            if let Some(min_ram) = self.ram_min_gb {
                if rec_ram < min_ram {
                    return Err("Recommended RAM cannot be less than minimum RAM".into());
                }
            }
        }
        if let Some(vram) = self.vram_min_gb {
            if !vram.is_finite() || vram <= 0.0 {
                return Err("Minimum VRAM must be a positive finite number".into());
            }
        }
        if let Some(rec_vram) = self.vram_recommended_gb {
            if !rec_vram.is_finite() || rec_vram <= 0.0 {
                return Err("Recommended VRAM must be a positive finite number".into());
            }
            if let Some(min_vram) = self.vram_min_gb {
                if rec_vram < min_vram {
                    return Err("Recommended VRAM cannot be less than minimum VRAM".into());
                }
            }
        }
        Ok(())
    }
}

/// Supported export formats for an engine.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum OutputFormat {
    Glb,
    Stl,
    Obj,
}

/// Operational capabilities of an engine.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EngineCapabilities {
    pub progress_reporting: bool,
    pub cancellation: bool,
    pub refinement: bool,
    pub supported_output_formats: Vec<OutputFormat>,
}

impl Default for EngineCapabilities {
    fn default() -> Self {
        Self {
            progress_reporting: true,
            cancellation: true,
            refinement: false,
            supported_output_formats: vec![OutputFormat::Glb, OutputFormat::Stl],
        }
    }
}

impl EngineCapabilities {
    pub fn validate(&self) -> Result<(), String> {
        if self.supported_output_formats.is_empty() {
            return Err("Engine must support at least one output format".into());
        }
        Ok(())
    }
}

/// Explicit validation status per target.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TargetValidationStatus {
    Validated,
    Supported,
    Experimental,
    CompatibleUnverified,
    Unsupported,
}

impl TargetValidationStatus {
    pub fn is_validated(self) -> bool {
        matches!(self, Self::Validated | Self::Supported)
    }
}

/// Fine-grained compatibility states per roadmap Section 8.2.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CompatibilityStatus {
    Supported,
    CompatibleUnverified,
    Experimental,
    InsufficientResources,
    RuntimeMissing,
    Unsupported,
}

impl CompatibilityStatus {
    #[allow(dead_code)]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Supported => "supported",
            Self::CompatibleUnverified => "compatible_unverified",
            Self::Experimental => "experimental",
            Self::InsufficientResources => "insufficient_resources",
            Self::RuntimeMissing => "runtime_missing",
            Self::Unsupported => "unsupported",
        }
    }
}

/// Resource preflight results evaluating physical RAM, recommended RAM, and disk footprint.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ResourcePreflight {
    pub passed: bool,
    pub ram_passed: bool,
    pub storage_passed: bool,
    pub ram_detected_gb: f64,
    pub ram_minimum_gb: Option<f64>,
    pub ram_recommended_gb: Option<f64>,
    pub storage_detected_gb: Option<f64>,
    pub storage_required_gb: Option<f64>,
    pub details: Vec<String>,
}

impl ResourcePreflight {
    pub fn evaluate(
        hardware: &HardwareProfile,
        manifest: &EngineManifest,
        target: Option<&EngineTarget>,
    ) -> Self {
        let ram_detected_gb = hardware.memory_gb;
        let ram_minimum_gb = target.and_then(|t| t.minimum_ram_gb).or_else(|| {
            manifest
                .resource_thresholds
                .as_ref()
                .and_then(|r| r.ram_min_gb)
        });
        let ram_recommended_gb = target.and_then(|t| t.recommended_ram_gb).or_else(|| {
            manifest
                .resource_thresholds
                .as_ref()
                .and_then(|r| r.ram_recommended_gb)
        });

        let storage_detected_gb = hardware.storage_gb;
        let storage_required_gb = manifest
            .resource_thresholds
            .as_ref()
            .and_then(|r| r.installed_footprint_bytes.or(r.disk_download_bytes))
            .map(|b| (b as f64) / 1_000_000_000.0);

        let mut details = Vec::new();

        let ram_passed = if let Some(min_ram) = ram_minimum_gb {
            if !ram_detected_gb.is_finite() || ram_detected_gb < min_ram {
                details.push(format!(
                    "Detected RAM ({:.1} GB) is below required minimum ({:.1} GB).",
                    if ram_detected_gb.is_finite() {
                        ram_detected_gb
                    } else {
                        0.0
                    },
                    min_ram
                ));
                false
            } else {
                if let Some(rec_ram) = ram_recommended_gb {
                    if ram_detected_gb < rec_ram {
                        details.push(format!(
                            "Detected RAM ({:.1} GB) meets minimum ({:.1} GB) but is below recommended ({:.1} GB).",
                            ram_detected_gb, min_ram, rec_ram
                        ));
                    } else {
                        details.push(format!(
                            "Detected RAM ({:.1} GB) meets recommended specification ({:.1} GB).",
                            ram_detected_gb, rec_ram
                        ));
                    }
                } else {
                    details.push(format!(
                        "Detected RAM ({:.1} GB) meets minimum requirement ({:.1} GB).",
                        ram_detected_gb, min_ram
                    ));
                }
                true
            }
        } else {
            details.push(format!(
                "No minimum RAM specified; detected RAM is {:.1} GB.",
                if ram_detected_gb.is_finite() {
                    ram_detected_gb
                } else {
                    0.0
                }
            ));
            true
        };

        let storage_passed =
            if let (Some(req_gb), Some(det_gb)) = (storage_required_gb, storage_detected_gb) {
                if det_gb < req_gb {
                    details.push(format!(
                        "Detected storage ({:.1} GB) is below estimated footprint ({:.1} GB).",
                        det_gb, req_gb
                    ));
                    false
                } else {
                    details.push(format!(
                    "Detected storage ({:.1} GB) is sufficient for estimated footprint ({:.1} GB).",
                    det_gb, req_gb
                ));
                    true
                }
            } else {
                true
            };

        let passed = ram_passed && storage_passed;

        Self {
            passed,
            ram_passed,
            storage_passed,
            ram_detected_gb: if ram_detected_gb.is_finite() {
                ram_detected_gb
            } else {
                0.0
            },
            ram_minimum_gb,
            ram_recommended_gb,
            storage_detected_gb,
            storage_required_gb,
            details,
        }
    }
}

/// Detailed dynamic evaluation of an engine manifest against detected hardware.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct EngineEvaluation {
    pub engine_id: String,
    pub compatibility: String,
    pub compatibility_status: CompatibilityStatus,
    pub reason: String,
    pub recommendation_reason: Option<String>,
    pub requires_confirmation: bool,
    pub requires_cpu_confirmation: bool,
    pub confirmation_reason: Option<String>,
    pub resource_preflight: ResourcePreflight,
    pub candidate_target: Option<EngineTarget>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EngineTarget {
    pub platform: String,
    pub architecture: String,
    pub compute_backend: String,
    pub device: ComputeDevice,
    /// Physical RAM admission floor; not a VRAM measurement or peak estimate.
    #[serde(default)]
    pub minimum_ram_gb: Option<f64>,
    #[serde(default)]
    pub recommended_ram_gb: Option<f64>,
    #[serde(default)]
    pub minimum_vram_gb: Option<f64>,
    #[serde(default)]
    pub requires_apple_silicon: bool,
    #[serde(default)]
    pub validated: bool,
    #[serde(default)]
    pub validation_status: Option<TargetValidationStatus>,
}

impl EngineTarget {
    pub fn is_target_validated(&self) -> bool {
        if let Some(status) = self.validation_status {
            status.is_validated()
        } else {
            self.validated
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if !["macos", "windows", "linux", "any"].contains(&self.platform.as_str()) {
            return Err(format!("Unsupported target platform: {}", self.platform));
        }
        if !["arm64", "x86_64", "any"].contains(&self.architecture.as_str()) {
            return Err(format!(
                "Unsupported target architecture: {}",
                self.architecture
            ));
        }
        if !["Metal", "CUDA", "Vulkan", "DirectML", "CPU"].contains(&self.compute_backend.as_str())
        {
            return Err(format!(
                "Unsupported compute backend: {}",
                self.compute_backend
            ));
        }
        if self.requires_apple_silicon && self.platform != "macos" {
            return Err("requiresAppleSilicon can only be set for macOS targets".into());
        }
        if let Some(gb) = self.minimum_ram_gb {
            if !gb.is_finite() || gb <= 0.0 {
                return Err("minimumRamGb must be a positive finite number".into());
            }
        }
        if let Some(rec) = self.recommended_ram_gb {
            if !rec.is_finite() || rec <= 0.0 {
                return Err("recommendedRamGb must be a positive finite number".into());
            }
            if let Some(min) = self.minimum_ram_gb {
                if rec < min {
                    return Err("recommendedRamGb cannot be less than minimumRamGb".into());
                }
            }
        }
        if let Some(vram) = self.minimum_vram_gb {
            if !vram.is_finite() || vram <= 0.0 {
                return Err("minimumVramGb must be a positive finite number".into());
            }
        }
        match self.device {
            ComputeDevice::Mps => {
                if self.platform != "macos" || self.compute_backend != "Metal" {
                    return Err("MPS compute device requires macOS and Metal backend".into());
                }
            }
            ComputeDevice::Cuda => {
                if self.compute_backend != "CUDA" {
                    return Err("CUDA compute device requires CUDA backend".into());
                }
            }
            ComputeDevice::Vulkan => {
                if self.compute_backend != "Vulkan" {
                    return Err("Vulkan compute device requires Vulkan backend".into());
                }
            }
            ComputeDevice::Directml => {
                if self.compute_backend != "DirectML"
                    || (self.platform != "windows" && self.platform != "any")
                {
                    return Err(
                        "DirectML compute device requires DirectML backend and Windows platform"
                            .into(),
                    );
                }
            }
            ComputeDevice::Cpu => {
                if self.compute_backend != "CPU" {
                    return Err("CPU compute device requires CPU backend".into());
                }
            }
        }
        if let Some(status) = self.validation_status {
            if self.validated && status == TargetValidationStatus::Unsupported {
                return Err(
                    "Target cannot have validated=true and validationStatus=unsupported".into(),
                );
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EngineManifest {
    pub schema_version: u32,
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub publisher: Option<String>,
    #[serde(default)]
    pub upstream_url: Option<String>,
    /// Component license metadata, not a license audit of its dependencies.
    pub license_identifier: String,
    #[serde(default)]
    pub engine_revision: Option<String>,
    #[serde(default)]
    pub runtime_spec_path: Option<String>,
    #[serde(default)]
    pub adapter: Option<RuntimeAdapter>,
    pub implemented: bool,
    pub native_hardware_required: bool,
    #[serde(default = "default_supported_inputs")]
    pub supported_inputs: Vec<InputModality>,
    #[serde(default)]
    pub material_capabilities: MaterialCapabilities,
    #[serde(default)]
    pub resource_thresholds: Option<ResourceThresholds>,
    #[serde(default)]
    pub capabilities: EngineCapabilities,
    pub targets: Vec<EngineTarget>,
    #[serde(default)]
    pub known_limits: Vec<String>,
    #[serde(default)]
    pub notes: Option<String>,
}

#[allow(dead_code)]
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
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

    pub fn validate(&self) -> Result<(), String> {
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
        if self.implemented && self.supported_inputs.is_empty() {
            return Err("An implemented engine must support at least one input modality".into());
        }
        self.material_capabilities.validate()?;
        if let Some(thresholds) = &self.resource_thresholds {
            thresholds.validate()?;
        }
        self.capabilities.validate()?;

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
            target.validate()?;
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
            "schemaVersion": 1,
            "id": "demo",
            "name": "Workspace Demo",
            "description": "Procedural sample sculpture demonstrating viewport and controls without AI inference.",
            "publisher": "Sculpt Project",
            "upstreamUrl": null,
            "licenseIdentifier": "MIT",
            "engineRevision": null,
            "runtimeSpecPath": null,
            "adapter": "demo",
            "implemented": true,
            "nativeHardwareRequired": false,
            "supportedInputs": ["single_image"],
            "materialCapabilities": {
                "channels": ["vertex_color"],
                "claimsPbr": false
            },
            "resourceThresholds": null,
            "capabilities": {
                "progressReporting": true,
                "cancellation": true,
                "refinement": false,
                "supportedOutputFormats": ["glb", "stl"]
            },
            "targets": [
                {
                    "platform": "any",
                    "architecture": "any",
                    "computeBackend": "CPU",
                    "device": "cpu",
                    "minimumRamGb": null,
                    "requiresAppleSilicon": false,
                    "validated": true,
                    "validationStatus": "validated"
                }
            ],
            "knownLimits": [
                "Generates a static procedural knot rather than reconstructing input images."
            ],
            "notes": "Zero-dependency fallback demo for UI testing and navigation."
        }"#,
        )
        .expect("Valid bundled demo descriptor");
        let trellis = EngineManifest::parse(
            r#"{
            "schemaVersion": 1,
            "id": "trellis-2",
            "name": "TRELLIS.2",
            "description": "A demanding engine profile reserved for a future supported runtime.",
            "publisher": "Microsoft",
            "upstreamUrl": "https://github.com/microsoft/TRELLIS.2",
            "licenseIdentifier": "MIT",
            "engineRevision": null,
            "runtimeSpecPath": null,
            "adapter": null,
            "implemented": false,
            "nativeHardwareRequired": true,
            "supportedInputs": ["single_image", "text_prompt"],
            "materialCapabilities": {
                "channels": ["diffuse_color", "normal", "roughness", "metallic"],
                "claimsPbr": true
            },
            "resourceThresholds": {
                "ramMinGb": 32.0,
                "ramRecommendedGb": 64.0,
                "vramMinGb": 24.0,
                "vramRecommendedGb": 24.0,
                "diskDownloadBytes": 15000000000,
                "installedFootprintBytes": 30000000000
            },
            "capabilities": {
                "progressReporting": true,
                "cancellation": true,
                "refinement": true,
                "supportedOutputFormats": ["glb", "stl", "obj"]
            },
            "targets": [
                {
                    "platform": "linux",
                    "architecture": "x86_64",
                    "computeBackend": "CUDA",
                    "device": "cuda",
                    "minimumRamGb": 32.0,
                    "recommendedRamGb": 64.0,
                    "minimumVramGb": 24.0,
                    "requiresAppleSilicon": false,
                    "validated": false,
                    "validationStatus": "experimental"
                }
            ],
            "knownLimits": [
                "Requires minimum 24GB VRAM GPU."
            ],
            "notes": "No validated runtime or model adapter is included. Not recommended for this Mac; installation and generation are unavailable."
        }"#,
        )
        .expect("Valid bundled TRELLIS.2 descriptor");
        vec![triposr, demo, trellis]
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
        .filter(|target| target.is_target_validated())
        .collect();
    for target in validated.iter().copied().filter(matches_backend) {
        let ram_threshold = target.minimum_ram_gb.or_else(|| {
            manifest
                .resource_thresholds
                .as_ref()
                .and_then(|r| r.ram_min_gb)
        });
        if let Some(minimum) = ram_threshold {
            if !hardware.memory_gb.is_finite() || hardware.memory_gb < minimum {
                return Err(format!(
                    "This validated runtime requires at least {minimum} GB of detected system memory."
                ));
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
    Err(
        "This hardware has no validated Sculpt runtime for this engine. Installation and generation are unavailable."
            .into(),
    )
}

/// Dynamically evaluate an engine manifest against detected hardware,
/// separating detected hardware, declared compatibility, validated support,
/// fine-grained compatibility states, and resource preflight.
pub fn evaluate_manifest(
    hardware: &HardwareProfile,
    manifest: &EngineManifest,
) -> EngineEvaluation {
    if !manifest.implemented {
        let reason = manifest.notes.clone().unwrap_or_else(|| {
            "No validated runtime adapter is included for this engine. Installation and generation are unavailable.".into()
        });
        return EngineEvaluation {
            engine_id: manifest.id.clone(),
            compatibility: "unsupported".into(),
            compatibility_status: CompatibilityStatus::Unsupported,
            reason,
            recommendation_reason: None,
            requires_confirmation: false,
            requires_cpu_confirmation: false,
            confirmation_reason: None,
            resource_preflight: ResourcePreflight::evaluate(hardware, manifest, None),
            candidate_target: None,
        };
    }

    if manifest.native_hardware_required && hardware.detection_source != "native" {
        return EngineEvaluation {
            engine_id: manifest.id.clone(),
            compatibility: "unsupported".into(),
            compatibility_status: CompatibilityStatus::Unsupported,
            reason: "Open the desktop app to detect this machine and use local reconstruction."
                .into(),
            recommendation_reason: Some(
                "Desktop application required for local hardware acceleration.".into(),
            ),
            requires_confirmation: false,
            requires_cpu_confirmation: false,
            confirmation_reason: None,
            resource_preflight: ResourcePreflight::evaluate(hardware, manifest, None),
            candidate_target: None,
        };
    }

    let matching_platform_targets: Vec<&EngineTarget> = manifest
        .targets
        .iter()
        .filter(|target| platform_matches(target, hardware))
        .collect();

    if matching_platform_targets.is_empty() {
        return EngineEvaluation {
            engine_id: manifest.id.clone(),
            compatibility: "unsupported".into(),
            compatibility_status: CompatibilityStatus::Unsupported,
            reason: "This hardware has no validated Sculpt runtime for this engine. Installation and generation are unavailable.".into(),
            recommendation_reason: None,
            requires_confirmation: false,
            requires_cpu_confirmation: false,
            confirmation_reason: None,
            resource_preflight: ResourcePreflight::evaluate(hardware, manifest, None),
            candidate_target: None,
        };
    }

    // Filter candidate targets where hardware has the required compute backend
    let matching_backend_targets: Vec<&EngineTarget> = matching_platform_targets
        .iter()
        .copied()
        .filter(|target| {
            hardware
                .compute_backends
                .iter()
                .any(|b| b == &target.compute_backend)
        })
        .collect();

    // Filter out explicitly unsupported targets (e.g. TripoSR CPU target which has validationStatus == Unsupported)
    let usable_targets: Vec<&EngineTarget> = matching_backend_targets
        .into_iter()
        .filter(|target| target.validation_status != Some(TargetValidationStatus::Unsupported))
        .collect();

    if usable_targets.is_empty() {
        // If there were platform-matching targets that are validated, but compute backend was missing
        let reason = if matching_platform_targets
            .iter()
            .any(|t| t.is_target_validated())
        {
            "The compute backend required by this validated runtime was not detected.".into()
        } else {
            "This hardware has no validated Sculpt runtime for this engine. Installation and generation are unavailable.".into()
        };
        return EngineEvaluation {
            engine_id: manifest.id.clone(),
            compatibility: "unsupported".into(),
            compatibility_status: CompatibilityStatus::Unsupported,
            reason,
            recommendation_reason: None,
            requires_confirmation: false,
            requires_cpu_confirmation: false,
            confirmation_reason: None,
            resource_preflight: ResourcePreflight::evaluate(hardware, manifest, None),
            candidate_target: None,
        };
    }

    // Select the best candidate target:
    // Prioritize GPU (non-CPU) targets over CPU targets; then prioritize Validated > CompatibleUnverified > Experimental
    let best_target = usable_targets
        .into_iter()
        .min_by_key(|t| {
            let is_cpu = t.device == ComputeDevice::Cpu;
            let tier = match t.validation_status.unwrap_or(if t.validated {
                TargetValidationStatus::Validated
            } else {
                TargetValidationStatus::Experimental
            }) {
                TargetValidationStatus::Validated | TargetValidationStatus::Supported => 0,
                TargetValidationStatus::CompatibleUnverified => 1,
                TargetValidationStatus::Experimental => 2,
                TargetValidationStatus::Unsupported => 99,
            };
            (if is_cpu { 10 } else { 0 }) + tier
        })
        .expect("At least one usable target exists");

    // Perform resource preflight check on best target
    let preflight = ResourcePreflight::evaluate(hardware, manifest, Some(best_target));

    if !preflight.ram_passed {
        let min_gb = preflight.ram_minimum_gb.unwrap_or(16.0);
        return EngineEvaluation {
            engine_id: manifest.id.clone(),
            compatibility: "unsupported".into(),
            compatibility_status: CompatibilityStatus::InsufficientResources,
            reason: format!(
                "This validated runtime requires at least {min_gb} GB of detected system memory."
            ),
            recommendation_reason: Some(format!(
                "Detected memory ({:.1} GB) is below minimum admission threshold ({:.1} GB).",
                hardware.memory_gb, min_gb
            )),
            requires_confirmation: false,
            requires_cpu_confirmation: false,
            confirmation_reason: None,
            resource_preflight: preflight,
            candidate_target: Some(best_target.clone()),
        };
    }

    if !preflight.storage_passed {
        let req_gb = preflight.storage_required_gb.unwrap_or(0.0);
        return EngineEvaluation {
            engine_id: manifest.id.clone(),
            compatibility: "unsupported".into(),
            compatibility_status: CompatibilityStatus::InsufficientResources,
            reason: format!(
                "Insufficient disk space: engine requires {req_gb:.1} GB free storage."
            ),
            recommendation_reason: Some(
                "Free disk space is below required installation footprint.".into(),
            ),
            requires_confirmation: false,
            requires_cpu_confirmation: false,
            confirmation_reason: None,
            resource_preflight: preflight,
            candidate_target: Some(best_target.clone()),
        };
    }

    // CPU fallback confirmation policy:
    // CPU fallback must be explicitly selected/confirmed, supported by that engine, and resource-gated.
    // NEVER silently downgrade from GPU to CPU.
    let is_cpu_target = best_target.device == ComputeDevice::Cpu;
    let is_inference_engine = manifest.adapter != Some(RuntimeAdapter::Demo);
    let (requires_confirmation, requires_cpu_confirmation, confirmation_reason) = if is_cpu_target
        && is_inference_engine
    {
        (
                true,
                true,
                Some(
                    "CPU execution is significantly slower than GPU acceleration and may cause high CPU and thermal load. Explicit confirmation is required to run on CPU."
                        .into(),
                ),
            )
    } else {
        (false, false, None)
    };

    // Determine fine-grained compatibility status based on target validation status
    let val_status = best_target
        .validation_status
        .unwrap_or(if best_target.validated {
            TargetValidationStatus::Validated
        } else {
            TargetValidationStatus::Experimental
        });

    match val_status {
        TargetValidationStatus::Validated | TargetValidationStatus::Supported => EngineEvaluation {
            engine_id: manifest.id.clone(),
            compatibility: if manifest.id == "triposr" {
                "recommended".into()
            } else {
                "available".into()
            },
            compatibility_status: CompatibilityStatus::Supported,
            reason: if manifest.id == "triposr" {
                "Runs locally through an isolated Python worker. Best with one clearly visible object. Unseen surfaces are inferred; quality varies. One-time runtime setup is required.".into()
            } else if manifest.id == "demo" {
                "An explicit demonstration of the workspace. Output is a procedural sculpture, not an AI reconstruction.".into()
            } else {
                "A validated adapter is available; runtime installation is checked separately.".into()
            },
            recommendation_reason: Some(format!("Validated for {} acceleration.", best_target.compute_backend)),
            requires_confirmation,
            requires_cpu_confirmation,
            confirmation_reason,
            resource_preflight: preflight,
            candidate_target: Some(best_target.clone()),
        },
        TargetValidationStatus::CompatibleUnverified => EngineEvaluation {
            engine_id: manifest.id.clone(),
            compatibility: "available".into(),
            compatibility_status: CompatibilityStatus::CompatibleUnverified,
            reason: "Hardware meets requirements, but this target has not been validated end-to-end yet.".into(),
            recommendation_reason: Some(
                "Unverified compatibility: may run, but stability and quality have not been confirmed.".into(),
            ),
            requires_confirmation,
            requires_cpu_confirmation,
            confirmation_reason,
            resource_preflight: preflight,
            candidate_target: Some(best_target.clone()),
        },
        TargetValidationStatus::Experimental => EngineEvaluation {
            engine_id: manifest.id.clone(),
            compatibility: "available".into(),
            compatibility_status: CompatibilityStatus::Experimental,
            reason: "Experimental support: opt-in inference with known limitations.".into(),
            recommendation_reason: Some(
                "Experimental target: intended for testing; unexpected errors or quality degradation may occur.".into(),
            ),
            requires_confirmation,
            requires_cpu_confirmation,
            confirmation_reason,
            resource_preflight: preflight,
            candidate_target: Some(best_target.clone()),
        },
        TargetValidationStatus::Unsupported => EngineEvaluation {
            engine_id: manifest.id.clone(),
            compatibility: "unsupported".into(),
            compatibility_status: CompatibilityStatus::Unsupported,
            reason: "This hardware has no validated Sculpt runtime for this engine. Installation and generation are unavailable.".into(),
            recommendation_reason: None,
            requires_confirmation: false,
            requires_cpu_confirmation: false,
            confirmation_reason: None,
            resource_preflight: preflight,
            candidate_target: Some(best_target.clone()),
        },
    }
}

/// Pure planning: no downloads, process launches, filesystem reads or fallback.
#[allow(dead_code)]
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
            npu: Some("Apple Neural Engine".into()),
        }
    }

    #[test]
    fn m4_resolves_the_validated_compiled_adapter() {
        let selected = resolve("triposr", &m4()).unwrap();
        assert_eq!(selected.adapter, RuntimeAdapter::TripoSr);
        assert_eq!(selected.device, ComputeDevice::Mps);
        assert_eq!(selected.device.as_str(), "mps");
        assert_eq!(selected.manifest.id, "triposr");
        assert_eq!(
            selected.manifest.supported_inputs,
            vec![InputModality::SingleImage]
        );
        assert_eq!(
            selected.manifest.material_capabilities.channels,
            vec![MaterialChannel::VertexColor, MaterialChannel::DiffuseColor]
        );
        assert!(!selected.manifest.material_capabilities.claims_pbr);
        assert!(selected.manifest.capabilities.progress_reporting);
        assert!(selected.manifest.capabilities.cancellation);
        assert!(selected.manifest.capabilities.refinement);
        assert_eq!(
            selected.manifest.capabilities.supported_output_formats,
            vec![OutputFormat::Glb, OutputFormat::Stl]
        );
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
    fn hardware_detection_does_not_imply_model_support() {
        // Case 1: Windows x64 with high-end NVIDIA GPU (CUDA detected)
        let win_cuda = HardwareProfile {
            os: "windows".into(),
            os_version: "10.0.26100".into(),
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
        let err = resolve("triposr", &win_cuda).unwrap_err();
        assert!(err.contains("no validated Sculpt runtime"));
        let plan_res = plan(&win_cuda, manifests());
        let triposr_plan = plan_res.iter().find(|e| e.engine_id == "triposr").unwrap();
        assert!(!triposr_plan.available);

        // Case 2: Windows ARM64 Snapdragon X Elite with Qualcomm NPU and Vulkan
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
        assert!(resolve("triposr", &win_arm).is_err());

        // Case 3: Linux x64 with NVIDIA H100
        let linux_cuda = HardwareProfile {
            os: "linux".into(),
            os_version: "Ubuntu 22.04".into(),
            architecture: "x86_64".into(),
            chip: "Intel Xeon".into(),
            gpu: "NVIDIA H100".into(),
            memory_gb: 128.0,
            unified_memory: false,
            is_apple_silicon: false,
            compute_backends: vec!["CUDA".into(), "CPU".into()],
            detection_source: "native".into(),
            storage_gb: Some(2048.0),
            npu: None,
        };
        assert!(resolve("triposr", &linux_cuda).is_err());

        // Case 4: Powerful CPU-only workstation does NOT grant CPU inference for TripoSR
        let cpu_only = HardwareProfile {
            os: "windows".into(),
            os_version: "11".into(),
            architecture: "x86_64".into(),
            chip: "AMD Threadripper 7980X".into(),
            gpu: "Basic Display Adapter".into(),
            memory_gb: 128.0,
            unified_memory: false,
            is_apple_silicon: false,
            compute_backends: vec!["CPU".into()],
            detection_source: "native".into(),
            storage_gb: Some(2048.0),
            npu: None,
        };
        assert!(resolve("triposr", &cpu_only).is_err());
        // But Demo is supported on CPU!
        let demo_sel = resolve("demo", &cpu_only).unwrap();
        assert_eq!(demo_sel.adapter, RuntimeAdapter::Demo);
        assert_eq!(demo_sel.device, ComputeDevice::Cpu);
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

    #[test]
    fn pbr_validation_enforces_material_channel_truth() {
        // Case 1: Claims PBR but only has vertex color and diffuse -> must FAIL
        let json = include_str!("../../../backend/engines/triposr.json");
        let mut value: serde_json::Value = serde_json::from_str(json).unwrap();
        value["materialCapabilities"]["claimsPbr"] = serde_json::json!(true);
        let err = EngineManifest::parse(&value.to_string()).unwrap_err();
        assert!(err.contains("Full PBR cannot be claimed"));

        // Case 2: Claims PBR with complete channel set -> must SUCCEED
        value["materialCapabilities"]["channels"] =
            serde_json::json!(["diffuse_color", "normal", "roughness", "metallic"]);
        let manifest = EngineManifest::parse(&value.to_string()).unwrap();
        assert!(manifest.material_capabilities.claims_pbr);
        assert!(manifest.material_capabilities.has_full_pbr());

        // Case 3: Empty channels -> must FAIL
        value["materialCapabilities"]["channels"] = serde_json::json!([]);
        value["materialCapabilities"]["claimsPbr"] = serde_json::json!(false);
        assert!(EngineManifest::parse(&value.to_string())
            .unwrap_err()
            .contains("at least one output channel"));
    }

    #[test]
    fn resource_thresholds_validation() {
        let json = include_str!("../../../backend/engines/triposr.json");
        let mut value: serde_json::Value = serde_json::from_str(json).unwrap();

        // Recommended RAM < Minimum RAM -> FAIL
        value["resourceThresholds"]["ramMinGb"] = serde_json::json!(16.0);
        value["resourceThresholds"]["ramRecommendedGb"] = serde_json::json!(8.0);
        assert!(EngineManifest::parse(&value.to_string())
            .unwrap_err()
            .contains("Recommended RAM cannot be less"));

        // Negative VRAM -> FAIL
        value["resourceThresholds"]["ramRecommendedGb"] = serde_json::json!(16.0);
        value["resourceThresholds"]["vramMinGb"] = serde_json::json!(-4.0);
        assert!(EngineManifest::parse(&value.to_string())
            .unwrap_err()
            .contains("Minimum VRAM must be a positive finite number"));
    }

    #[test]
    fn target_validation_and_consistency() {
        let json = include_str!("../../../backend/engines/triposr.json");
        let mut value: serde_json::Value = serde_json::from_str(json).unwrap();

        // Mismatched device and backend: Cuda with Metal backend -> FAIL
        value["targets"][0]["computeBackend"] = serde_json::json!("Metal");
        value["targets"][0]["device"] = serde_json::json!("cuda");
        assert!(EngineManifest::parse(&value.to_string())
            .unwrap_err()
            .contains("CUDA compute device requires CUDA backend"));

        // DirectML on macOS -> FAIL
        value["targets"][0]["computeBackend"] = serde_json::json!("DirectML");
        value["targets"][0]["device"] = serde_json::json!("directml");
        assert!(EngineManifest::parse(&value.to_string())
            .unwrap_err()
            .contains("DirectML compute device requires DirectML backend and Windows platform"));

        // validated=true but validationStatus=unsupported -> FAIL
        value["targets"][0]["computeBackend"] = serde_json::json!("Metal");
        value["targets"][0]["device"] = serde_json::json!("mps");
        value["targets"][0]["validated"] = serde_json::json!(true);
        value["targets"][0]["validationStatus"] = serde_json::json!("unsupported");
        assert!(EngineManifest::parse(&value.to_string())
            .unwrap_err()
            .contains("validated=true and validationStatus=unsupported"));
    }

    #[test]
    fn roundtrip_serialization_and_backward_compatibility() {
        // Roundtrip bundled TripoSR
        let triposr_json = include_str!("../../../backend/engines/triposr.json");
        let parsed = EngineManifest::parse(triposr_json).unwrap();
        let serialized = serde_json::to_string_pretty(&parsed).unwrap();
        let roundtrip = EngineManifest::parse(&serialized).unwrap();
        assert_eq!(parsed, roundtrip);

        // Minimal legacy descriptor without new optional fields
        let legacy_json = r#"{
            "schemaVersion": 1,
            "id": "demo",
            "name": "Legacy Demo",
            "licenseIdentifier": "MIT",
            "adapter": "demo",
            "implemented": true,
            "nativeHardwareRequired": false,
            "targets": [
                {
                    "platform": "any",
                    "architecture": "any",
                    "computeBackend": "CPU",
                    "device": "cpu",
                    "requiresAppleSilicon": false,
                    "validated": true
                }
            ]
        }"#;
        let legacy_manifest = EngineManifest::parse(legacy_json).unwrap();
        assert_eq!(legacy_manifest.id, "demo");
        assert_eq!(
            legacy_manifest.supported_inputs,
            vec![InputModality::SingleImage]
        );
        assert_eq!(
            legacy_manifest.material_capabilities.channels,
            vec![MaterialChannel::VertexColor]
        );
        assert!(!legacy_manifest.material_capabilities.claims_pbr);
        assert!(legacy_manifest.capabilities.progress_reporting);
        assert_eq!(
            legacy_manifest.capabilities.supported_output_formats,
            vec![OutputFormat::Glb, OutputFormat::Stl]
        );
        assert!(legacy_manifest.targets[0].is_target_validated());
    }

    #[test]
    fn unimplemented_engines_cannot_be_resolved() {
        let manifest = EngineManifest {
            schema_version: 1,
            id: "trellis-2".into(),
            name: "TRELLIS.2".into(),
            description: Some("Advanced 3D asset generation".into()),
            publisher: Some("Microsoft".into()),
            upstream_url: Some("https://github.com/microsoft/TRELLIS.2".into()),
            license_identifier: "MIT".into(),
            engine_revision: None,
            runtime_spec_path: None,
            adapter: None,
            implemented: false,
            native_hardware_required: true,
            supported_inputs: vec![InputModality::SingleImage, InputModality::TextPrompt],
            material_capabilities: MaterialCapabilities {
                channels: vec![
                    MaterialChannel::DiffuseColor,
                    MaterialChannel::Normal,
                    MaterialChannel::Roughness,
                    MaterialChannel::Metallic,
                ],
                claims_pbr: true,
            },
            resource_thresholds: Some(ResourceThresholds {
                ram_min_gb: Some(32.0),
                ram_recommended_gb: Some(64.0),
                vram_min_gb: Some(24.0),
                vram_recommended_gb: Some(24.0),
                disk_download_bytes: Some(15000000000),
                installed_footprint_bytes: Some(30000000000),
            }),
            capabilities: EngineCapabilities {
                progress_reporting: true,
                cancellation: true,
                refinement: true,
                supported_output_formats: vec![
                    OutputFormat::Glb,
                    OutputFormat::Stl,
                    OutputFormat::Obj,
                ],
            },
            targets: vec![EngineTarget {
                platform: "linux".into(),
                architecture: "x86_64".into(),
                compute_backend: "CUDA".into(),
                device: ComputeDevice::Cuda,
                minimum_ram_gb: Some(32.0),
                recommended_ram_gb: Some(64.0),
                minimum_vram_gb: Some(24.0),
                requires_apple_silicon: false,
                validated: false,
                validation_status: Some(TargetValidationStatus::Experimental),
            }],
            known_limits: vec!["Requires minimum 24GB VRAM GPU".into()],
            notes: Some("Experimental future engine".into()),
        };
        assert!(manifest.validate().is_ok());

        // Attempting to plan this manifest on high-end hardware returns error
        let mut hardware = m4();
        hardware.os = "linux".into();
        hardware.architecture = "x86_64".into();
        hardware.compute_backends = vec!["CUDA".into()];
        hardware.memory_gb = 64.0;
        let err = plan_engine(&hardware, &manifest).unwrap_err();
        assert!(err.contains("No validated runtime adapter"));
    }
}
