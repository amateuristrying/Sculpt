//! Engine-independent asset manifest schema, provenance, and lineage contracts.
//!
//! Every completed 3D asset generated or refined by Sculpt is accompanied by an
//! engine-independent, versioned `AssetManifest` (`manifest.json`) in its job directory.
//! The manifest records the complete lineage, engine/model provenance, generation
//! parameters, geometry inventory, material/texture channels, and output artifacts
//! with SHA-256 integrity hashes.
//!
//! Manifests are engine-independent: an asset remains inspectable, exportable,
//! and reproducible even if the original inference runtime is uninstalled.

#![allow(dead_code)]

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const CURRENT_SCHEMA_VERSION: u32 = 1;

/// Supported reconstruction or modification operations.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssetOperation {
    Generate,
    Refine,
    Export,
    Import,
}

impl AssetOperation {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Generate => "generate",
            Self::Refine => "refine",
            Self::Export => "export",
            Self::Import => "import",
        }
    }
}

/// 3D coordinate system orientation conventions.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CoordinateSystem {
    #[serde(rename = "glTF Y-up")]
    GltfYUp,
    #[serde(rename = "Z-up")]
    ZUp,
    #[serde(rename = "Y-up")]
    YUp,
    #[serde(untagged)]
    Other(String),
}

impl CoordinateSystem {
    pub fn as_str(&self) -> &str {
        match self {
            Self::GltfYUp => "glTF Y-up",
            Self::ZUp => "Z-up",
            Self::YUp => "Y-up",
            Self::Other(s) => s.as_str(),
        }
    }
}

/// Physical scale unit declarations.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PhysicalUnit {
    Meters,
    Millimeters,
    Centimeters,
    Inches,
    Unitless,
    Unknown,
    #[serde(untagged)]
    Other(String),
}

impl PhysicalUnit {
    pub fn as_str(&self) -> &str {
        match self {
            Self::Meters => "meters",
            Self::Millimeters => "millimeters",
            Self::Centimeters => "centimeters",
            Self::Inches => "inches",
            Self::Unitless => "unitless",
            Self::Unknown => "unknown",
            Self::Other(s) => s.as_str(),
        }
    }
}

/// 3D Axis-Aligned Bounding Box (AABB) bounds in asset coordinate space.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BoundingBox {
    pub min: [f64; 3],
    pub max: [f64; 3],
}

impl BoundingBox {
    pub fn new(min: [f64; 3], max: [f64; 3]) -> Self {
        Self { min, max }
    }

    pub fn validate(&self) -> Result<(), String> {
        for i in 0..3 {
            if !self.min[i].is_finite() || !self.max[i].is_finite() {
                return Err("Bounding box coordinates must be finite numbers".into());
            }
            if self.min[i] > self.max[i] {
                return Err(format!(
                    "Bounding box min[{i}] ({}) cannot exceed max[{i}] ({})",
                    self.min[i], self.max[i]
                ));
            }
        }
        Ok(())
    }
}

/// Explicit material and texture channels.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ManifestMaterialChannel {
    VertexColor,
    DiffuseTexture,
    DiffuseColor,
    Normal,
    Roughness,
    Metallic,
    Ao,
    AmbientOcclusion,
    Emissive,
    Opacity,
    #[serde(untagged)]
    Other(String),
}

impl ManifestMaterialChannel {
    pub fn is_standard(&self) -> bool {
        !matches!(self, Self::Other(_))
    }
}

/// Texture color space encoding.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TextureColorSpace {
    #[serde(rename = "sRGB")]
    Srgb,
    #[serde(rename = "linear")]
    Linear,
    #[serde(rename = "unknown")]
    Unknown,
}

/// Material and texture inventory.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MaterialInventory {
    pub channels: Vec<ManifestMaterialChannel>,
    #[serde(default)]
    pub texture_width: Option<u32>,
    #[serde(default)]
    pub texture_height: Option<u32>,
    #[serde(default)]
    pub texture_color_space: Option<TextureColorSpace>,
    pub pbr: bool,
    #[serde(default)]
    pub unknown_channels: Vec<String>,
}

impl MaterialInventory {
    pub fn validate(&self) -> Result<(), String> {
        // Enforce PBR truthfulness: full PBR cannot be claimed unless complete channels exist.
        if self.pbr {
            let has_diffuse = self.channels.iter().any(|c| {
                matches!(
                    c,
                    ManifestMaterialChannel::DiffuseTexture
                        | ManifestMaterialChannel::DiffuseColor
                        | ManifestMaterialChannel::VertexColor
                )
            });
            let has_normal = self
                .channels
                .iter()
                .any(|c| matches!(c, ManifestMaterialChannel::Normal));
            let has_roughness = self
                .channels
                .iter()
                .any(|c| matches!(c, ManifestMaterialChannel::Roughness));
            let has_metallic = self
                .channels
                .iter()
                .any(|c| matches!(c, ManifestMaterialChannel::Metallic));

            if !has_diffuse || !has_normal || !has_roughness || !has_metallic {
                return Err("PBR status cannot be true unless diffuse/color, normal, roughness, and metallic channels are all present".into());
            }
        }

        if let (Some(w), Some(h)) = (self.texture_width, self.texture_height) {
            if w == 0 || h == 0 {
                return Err("Texture dimensions must be non-zero".into());
            }
        } else if self.texture_width.is_some() || self.texture_height.is_some() {
            return Err("Both textureWidth and textureHeight must be specified together".into());
        }

        Ok(())
    }
}

/// Output file artifact generated in the job directory.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OutputArtifact {
    pub path: String,
    pub mime_type: String,
    pub byte_length: u64,
    pub sha256: String,
}

impl OutputArtifact {
    pub fn validate(&self) -> Result<(), String> {
        if self.path.trim().is_empty() {
            return Err("Artifact path cannot be empty".into());
        }
        if self.path.starts_with('/')
            || self.path.starts_with('\\')
            || self.path.contains(':')
            || self.path.split(['/', '\\']).any(|p| p == "..")
        {
            return Err(format!(
                "Artifact path '{}' must be a safe relative path within the job directory",
                self.path
            ));
        }
        if self.mime_type.trim().is_empty() {
            return Err("Artifact mimeType cannot be empty".into());
        }
        if self.byte_length == 0 {
            return Err(format!(
                "Artifact '{}' byte length must be greater than zero",
                self.path
            ));
        }
        if !is_valid_sha256(&self.sha256) {
            return Err(format!(
                "Artifact '{}' has an invalid SHA-256 checksum: '{}'",
                self.path, self.sha256
            ));
        }
        Ok(())
    }
}

/// Structured non-fatal warning encountered during generation or validation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ValidationWarning {
    pub code: String,
    pub message: String,
}

/// Input images, masks, or text prompts used for generation.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SourceInputs {
    #[serde(default)]
    pub source_image_id: Option<String>,
    #[serde(default)]
    pub source_image_sha256: Option<String>,
    #[serde(default)]
    pub mask_id: Option<String>,
    #[serde(default)]
    pub mask_sha256: Option<String>,
    #[serde(default)]
    pub original_prompt: Option<String>,
    #[serde(default)]
    pub composed_prompt: Option<String>,
}

impl SourceInputs {
    pub fn validate(&self) -> Result<(), String> {
        if let Some(ref id) = self.source_image_id {
            if !is_valid_uuid(id) {
                return Err(format!("Invalid sourceImageId: '{}'", id));
            }
        }
        if let Some(ref sha) = self.source_image_sha256 {
            if !is_valid_sha256(sha) {
                return Err(format!("Invalid sourceImageSha256: '{}'", sha));
            }
        }
        if let Some(ref id) = self.mask_id {
            if !is_valid_uuid(id) {
                return Err(format!("Invalid maskId: '{}'", id));
            }
        }
        if let Some(ref sha) = self.mask_sha256 {
            if !is_valid_sha256(sha) {
                return Err(format!("Invalid maskSha256: '{}'", sha));
            }
        }
        if let Some(ref prompt) = self.original_prompt {
            if prompt.trim().is_empty() {
                return Err("originalPrompt cannot be an empty string".into());
            }
        }
        if let Some(ref prompt) = self.composed_prompt {
            if prompt.trim().is_empty() {
                return Err("composedPrompt cannot be an empty string".into());
            }
        }
        Ok(())
    }
}

/// Engine, runtime, and hardware provenance.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EngineProvenance {
    pub engine_id: String,
    #[serde(default)]
    pub engine_revision: Option<String>,
    #[serde(default)]
    pub model_weight_hashes: BTreeMap<String, String>,
    #[serde(default)]
    pub runtime_version: Option<String>,
    pub backend: String,
    pub device: String,
    #[serde(default)]
    pub driver_version: Option<String>,
}

impl EngineProvenance {
    pub fn validate(&self) -> Result<(), String> {
        if self.engine_id.trim().is_empty() {
            return Err("engineId cannot be empty".into());
        }
        if self.backend.trim().is_empty() {
            return Err("backend cannot be empty".into());
        }
        if self.device.trim().is_empty() {
            return Err("device cannot be empty".into());
        }
        for (name, hash) in &self.model_weight_hashes {
            if !is_valid_sha256(hash) {
                return Err(format!("Invalid weight SHA-256 for '{}': '{}'", name, hash));
            }
        }
        Ok(())
    }
}

/// Reconstruction and refinement parameters.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GenerationParameters {
    #[serde(default)]
    pub quality: Option<String>,
    #[serde(default)]
    pub seed: Option<u32>,
    #[serde(default)]
    pub resolution: Option<u32>,
    #[serde(default)]
    pub density_threshold: Option<f64>,
    #[serde(default)]
    pub smoothing_iterations: Option<u32>,
    #[serde(default)]
    pub remove_small_components: Option<bool>,
    #[serde(default)]
    pub target_face_count: Option<u32>,
    #[serde(default)]
    pub extra: BTreeMap<String, serde_json::Value>,
}

impl GenerationParameters {
    pub fn validate(&self) -> Result<(), String> {
        if let Some(density) = self.density_threshold {
            if !density.is_finite() || density < 0.0 {
                return Err("densityThreshold must be a non-negative finite number".into());
            }
        }
        if let Some(iters) = self.smoothing_iterations {
            if iters > 100 {
                return Err("smoothingIterations cannot exceed 100".into());
            }
        }
        if let Some(res) = self.resolution {
            if res == 0 {
                return Err("resolution must be greater than zero".into());
            }
        }
        if let Some(faces) = self.target_face_count {
            if faces == 0 {
                return Err("targetFaceCount must be greater than zero".into());
            }
        }
        Ok(())
    }
}

/// Asset ancestry and parent lineage.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AssetLineage {
    #[serde(default)]
    pub parent_asset_id: Option<String>,
    #[serde(default)]
    pub parent_asset_version: Option<u32>,
    pub operation: AssetOperation,
    #[serde(default)]
    pub ancestor_asset_ids: Vec<String>,
}

impl AssetLineage {
    pub fn validate(&self, asset_id: &str) -> Result<(), String> {
        if self.operation == AssetOperation::Refine {
            match self.parent_asset_id {
                Some(ref id) if !id.trim().is_empty() => {}
                _ => return Err("Parent asset ID is required when operation is refine".into()),
            }
        }
        if let Some(ref parent_id) = self.parent_asset_id {
            if !is_valid_uuid(parent_id) {
                return Err(format!("Invalid parentAssetId: '{}'", parent_id));
            }
            if parent_id == asset_id {
                return Err("Asset cannot be its own parent".into());
            }
        }
        for ancestor_id in &self.ancestor_asset_ids {
            if !is_valid_uuid(ancestor_id) {
                return Err(format!("Invalid ancestorAssetId: '{}'", ancestor_id));
            }
            if ancestor_id == asset_id {
                return Err("Asset cannot appear in its own ancestor chain".into());
            }
        }
        Ok(())
    }
}

/// Geometry metrics, bounding volume, and coordinate conventions.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GeometryInventory {
    pub vertex_count: u64,
    pub face_count: u64,
    #[serde(default)]
    pub component_count: Option<u32>,
    #[serde(default)]
    pub watertight: Option<bool>,
    pub bounding_box: BoundingBox,
    pub coordinate_system: CoordinateSystem,
    pub unit: PhysicalUnit,
}

impl GeometryInventory {
    pub fn validate(&self) -> Result<(), String> {
        self.bounding_box.validate()?;
        if self.face_count > 0 && self.vertex_count < 3 {
            return Err("Meshes with faces must have at least 3 vertices".into());
        }
        Ok(())
    }
}

/// Engine-independent, versioned manifest describing a generated or refined 3D asset.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AssetManifest {
    pub schema_version: u32,
    pub asset_id: String,
    pub created_at: String,
    pub source_inputs: SourceInputs,
    pub provenance: EngineProvenance,
    pub parameters: GenerationParameters,
    pub lineage: AssetLineage,
    pub geometry: GeometryInventory,
    pub materials: MaterialInventory,
    pub artifacts: Vec<OutputArtifact>,
    #[serde(default)]
    pub warnings: Vec<ValidationWarning>,
}

impl AssetManifest {
    pub fn validate(&self) -> Result<(), String> {
        if self.schema_version != CURRENT_SCHEMA_VERSION {
            return Err(format!(
                "Unsupported manifest schema version {}; current version is {}",
                self.schema_version, CURRENT_SCHEMA_VERSION
            ));
        }
        if !is_valid_uuid(&self.asset_id) {
            return Err(format!("Invalid assetId: '{}'", self.asset_id));
        }
        if !is_valid_rfc3339(&self.created_at) {
            return Err(format!(
                "Invalid createdAt timestamp (must be valid RFC 3339 / ISO 8601): '{}'",
                self.created_at
            ));
        }

        self.source_inputs.validate()?;
        self.provenance.validate()?;
        self.parameters.validate()?;
        self.lineage.validate(&self.asset_id)?;
        self.geometry.validate()?;
        self.materials.validate()?;

        if self.artifacts.is_empty() {
            return Err("Manifest must contain at least one output artifact".into());
        }

        let mut seen_paths = std::collections::BTreeSet::new();
        let mut has_primary_mesh = false;
        for artifact in &self.artifacts {
            artifact.validate()?;
            if !seen_paths.insert(&artifact.path) {
                return Err(format!("Duplicate artifact path: '{}'", artifact.path));
            }
            if artifact.path.ends_with(".glb")
                || artifact.path.ends_with(".stl")
                || artifact.path.ends_with(".obj")
            {
                has_primary_mesh = true;
            }
        }

        if !has_primary_mesh {
            return Err(
                "Manifest must reference at least one 3D model artifact (.glb, .stl, or .obj)"
                    .into(),
            );
        }

        Ok(())
    }

    /// Parse and validate an asset manifest from JSON string.
    pub fn parse(json_str: &str) -> Result<Self, String> {
        let manifest: Self = serde_json::from_str(json_str)
            .map_err(|e| format!("Invalid asset manifest JSON: {e}"))?;
        manifest.validate()?;
        Ok(manifest)
    }

    /// Validate and serialize this manifest to a pretty JSON string.
    pub fn to_json(&self) -> Result<String, String> {
        self.validate()?;
        serde_json::to_string_pretty(self)
            .map_err(|e| format!("Failed to serialize asset manifest: {e}"))
    }

    /// Migration policy: synthesize a valid `AssetManifest` from legacy v1 unmanifested metrics.
    ///
    /// Preserves existing metrics, populates geometry, material, and provenance defaults,
    /// and flags migration warnings for tracking.
    pub fn from_legacy_metrics(
        asset_id: &str,
        created_at: &str,
        metrics: &serde_json::Value,
        artifact: Option<OutputArtifact>,
    ) -> Result<Self, String> {
        if !is_valid_uuid(asset_id) {
            return Err(format!("Invalid assetId: '{asset_id}'"));
        }

        // Normalize created_at to RFC 3339 if given as epoch milliseconds.
        let normalized_created_at = if let Ok(epoch_ms) = created_at.parse::<u128>() {
            let secs = (epoch_ms / 1000) as u64;
            format_epoch_secs_to_rfc3339(secs)
        } else if is_valid_rfc3339(created_at) {
            created_at.to_string()
        } else {
            "1970-01-01T00:00:00Z".to_string()
        };

        let engine_id = metrics
            .get("engine")
            .and_then(|v| v.as_str())
            .unwrap_or("triposr")
            .to_string();

        let engine_revision = metrics
            .get("modelRevision")
            .and_then(|v| v.as_str())
            .map(String::from);

        let runtime_version = metrics
            .get("torchVersion")
            .and_then(|v| v.as_str())
            .map(|t| format!("pytorch-{t}"));

        let device = metrics
            .get("device")
            .and_then(|v| v.as_str())
            .unwrap_or("cpu")
            .to_string();

        let backend = match device.as_str() {
            "mps" => "Metal".to_string(),
            "cuda" => "CUDA".to_string(),
            _ => "CPU".to_string(),
        };

        let source_image_sha256 = metrics
            .get("sourceSha256")
            .and_then(|v| v.as_str())
            .map(String::from);

        let mask_sha256 = metrics
            .get("maskSha256")
            .and_then(|v| v.as_str())
            .map(String::from);

        let op_str = metrics
            .get("operation")
            .and_then(|v| v.as_str())
            .unwrap_or("generate");

        let operation = match op_str {
            "refine" => AssetOperation::Refine,
            _ => AssetOperation::Generate,
        };

        let parent_asset_id = metrics
            .get("parentAssetId")
            .or_else(|| metrics.get("parent_asset_id"))
            .and_then(|v| v.as_str())
            .map(String::from);

        let mut warnings = vec![ValidationWarning {
            code: "LEGACY_MIGRATION".into(),
            message: "Asset manifest was synthesized from legacy unmanifested metrics; missing fields were populated with defaults.".into(),
        }];

        // If legacy operation is refine but parent ID was not recorded in metrics, generate a synthetic UUID and warn.
        let effective_parent_id = if operation == AssetOperation::Refine {
            if let Some(ref pid) = parent_asset_id {
                Some(pid.clone())
            } else {
                warnings.push(ValidationWarning {
                    code: "MISSING_PARENT_ID_IN_LEGACY_REFINE".into(),
                    message: "Legacy refine metrics lacked parentAssetId; a fallback placeholder UUID was assigned to satisfy lineage contract.".into(),
                });
                Some("00000000-0000-0000-0000-000000000001".into())
            }
        } else {
            parent_asset_id
        };

        let quality = metrics
            .get("quality")
            .and_then(|v| v.as_str())
            .map(String::from);

        let resolution = metrics
            .get("refinement")
            .and_then(|r| r.get("resolution"))
            .and_then(|v| v.as_u64())
            .map(|v| v as u32);

        let density_threshold = metrics
            .get("refinement")
            .and_then(|r| r.get("densityThreshold"))
            .and_then(|v| v.as_f64());

        let smoothing_iterations = metrics
            .get("refinement")
            .and_then(|r| r.get("smoothingIterations"))
            .and_then(|v| v.as_u64())
            .map(|v| v as u32);

        let remove_small_components = metrics
            .get("refinement")
            .and_then(|r| r.get("removeSmallComponents"))
            .and_then(|v| v.as_bool());

        let target_face_count = metrics
            .get("refinement")
            .and_then(|r| r.get("targetFaceCount"))
            .and_then(|v| v.as_u64())
            .map(|v| v as u32);

        let vertex_count = metrics
            .get("vertices")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);

        let face_count = metrics.get("faces").and_then(|v| v.as_u64()).unwrap_or(0);

        let watertight = metrics
            .get("meshQuality")
            .and_then(|mq| mq.get("watertight"))
            .and_then(|v| v.as_bool());

        let component_count = metrics
            .get("meshQuality")
            .and_then(|mq| mq.get("components"))
            .and_then(|v| v.as_u64())
            .map(|v| v as u32);

        let texture_res = metrics
            .get("textureResolution")
            .and_then(|v| v.as_str())
            .unwrap_or("vertex");

        let channels = match texture_res {
            "1k" | "2k" => vec![ManifestMaterialChannel::DiffuseTexture],
            _ => vec![ManifestMaterialChannel::VertexColor],
        };

        let (texture_width, texture_height) = match texture_res {
            "1k" => (Some(1024), Some(1024)),
            "2k" => (Some(2048), Some(2048)),
            _ => (None, None),
        };

        let primary_artifact = if let Some(art) = artifact {
            art
        } else {
            let byte_length = metrics
                .get("fileBytes")
                .and_then(|v| v.as_u64())
                .unwrap_or(1024);
            let sha256 = metrics
                .get("sceneCacheSha256")
                .and_then(|v| v.as_str())
                .filter(|s| is_valid_sha256(s))
                .unwrap_or("e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855")
                .to_string();
            OutputArtifact {
                path: "mesh.glb".into(),
                mime_type: "model/gltf-binary".into(),
                byte_length,
                sha256,
            }
        };

        let manifest = Self {
            schema_version: CURRENT_SCHEMA_VERSION,
            asset_id: asset_id.to_string(),
            created_at: normalized_created_at,
            source_inputs: SourceInputs {
                source_image_id: None,
                source_image_sha256,
                mask_id: None,
                mask_sha256,
                original_prompt: None,
                composed_prompt: None,
            },
            provenance: EngineProvenance {
                engine_id,
                engine_revision,
                model_weight_hashes: BTreeMap::new(),
                runtime_version,
                backend,
                device,
                driver_version: None,
            },
            parameters: GenerationParameters {
                quality,
                seed: Some(0),
                resolution,
                density_threshold,
                smoothing_iterations,
                remove_small_components,
                target_face_count,
                extra: BTreeMap::new(),
            },
            lineage: AssetLineage {
                parent_asset_id: effective_parent_id,
                parent_asset_version: None,
                operation,
                ancestor_asset_ids: Vec::new(),
            },
            geometry: GeometryInventory {
                vertex_count,
                face_count,
                component_count,
                watertight,
                bounding_box: BoundingBox::new([-0.5, -0.5, -0.5], [0.5, 0.5, 0.5]),
                coordinate_system: CoordinateSystem::GltfYUp,
                unit: PhysicalUnit::Meters,
            },
            materials: MaterialInventory {
                channels,
                texture_width,
                texture_height,
                texture_color_space: if texture_width.is_some() {
                    Some(TextureColorSpace::Srgb)
                } else {
                    None
                },
                pbr: false, // Legacy TripoSR produces vertex color or unlit diffuse, never full PBR
                unknown_channels: Vec::new(),
            },
            artifacts: vec![primary_artifact],
            warnings,
        };

        manifest.validate()?;
        Ok(manifest)
    }
}

pub fn is_valid_uuid(id: &str) -> bool {
    if id.len() != 36 {
        return false;
    }
    match uuid::Uuid::parse_str(id) {
        Ok(parsed) => parsed.to_string() == id,
        Err(_) => false,
    }
}

pub fn is_valid_sha256(hash: &str) -> bool {
    hash.len() == 64
        && hash
            .chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
}

/// Minimal RFC 3339 / ISO 8601 UTC timestamp validator.
pub fn is_valid_rfc3339(timestamp: &str) -> bool {
    let t = timestamp.trim();
    if t.len() < 19 {
        return false;
    }
    // Expected structure: YYYY-MM-DDTHH:MM:SS[...]
    let bytes = t.as_bytes();
    if bytes[4] != b'-' || bytes[7] != b'-' || (bytes[10] != b'T' && bytes[10] != b't') {
        return false;
    }
    if bytes[13] != b':' || bytes[16] != b':' {
        return false;
    }
    // Verify numeric components
    let is_num = |b: u8| b.is_ascii_digit();
    if !bytes[0..4].iter().copied().all(is_num)
        || !bytes[5..7].iter().copied().all(is_num)
        || !bytes[8..10].iter().copied().all(is_num)
        || !bytes[11..13].iter().copied().all(is_num)
        || !bytes[14..16].iter().copied().all(is_num)
        || !bytes[17..19].iter().copied().all(is_num)
    {
        return false;
    }

    // Check timezone at end: 'Z' or 'z' or [+-]HH:MM
    let rem = &t[19..];
    if rem.is_empty() {
        return false;
    }
    if rem == "Z" || rem == "z" {
        return true;
    }
    if let Some(fraction_end) = rem.find(|c| c == 'Z' || c == 'z' || c == '+' || c == '-') {
        let (fraction, tz) = rem.split_at(fraction_end);
        if !fraction.starts_with('.') || !fraction[1..].chars().all(|c| c.is_ascii_digit()) {
            return false;
        }
        if tz == "Z" || tz == "z" {
            return true;
        }
        if tz.len() == 6 && (tz.starts_with('+') || tz.starts_with('-')) && tz.as_bytes()[3] == b':'
        {
            let tz_bytes = tz.as_bytes();
            return tz_bytes[1..3].iter().copied().all(is_num)
                && tz_bytes[4..6].iter().copied().all(is_num);
        }
        return false;
    }
    if rem.len() == 6 && (rem.starts_with('+') || rem.starts_with('-')) && rem.as_bytes()[3] == b':'
    {
        let tz_bytes = rem.as_bytes();
        return tz_bytes[1..3].iter().copied().all(is_num)
            && tz_bytes[4..6].iter().copied().all(is_num);
    }
    false
}

fn format_epoch_secs_to_rfc3339(epoch_secs: u64) -> String {
    // Days since unix epoch
    let mut days = epoch_secs / 86400;
    let day_secs = epoch_secs % 86400;
    let hours = day_secs / 3600;
    let mins = (day_secs % 3600) / 60;
    let secs = day_secs % 60;

    // Approximate year/month from days
    let mut year = 1970;
    loop {
        let leap = is_leap_year(year);
        let days_in_year = if leap { 366 } else { 365 };
        if days >= days_in_year {
            days -= days_in_year;
            year += 1;
        } else {
            break;
        }
    }
    let leap = is_leap_year(year);
    let month_days = [
        31,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    let mut month = 1;
    for &md in &month_days {
        if days >= md {
            days -= md;
            month += 1;
        } else {
            break;
        }
    }
    let day = days + 1;
    format!("{year:04}-{month:02}-{day:02}T{hours:02}:{mins:02}:{secs:02}Z")
}

fn is_leap_year(y: u64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || (y % 400 == 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_test_manifest() -> AssetManifest {
        AssetManifest {
            schema_version: 1,
            asset_id: "550e8400-e29b-41d4-a716-446655440000".into(),
            created_at: "2026-11-24T12:00:00Z".into(),
            source_inputs: SourceInputs {
                source_image_id: Some("550e8400-e29b-41d4-a716-446655440001".into()),
                source_image_sha256: Some(
                    "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855".into(),
                ),
                mask_id: None,
                mask_sha256: None,
                original_prompt: None,
                composed_prompt: None,
            },
            provenance: EngineProvenance {
                engine_id: "triposr".into(),
                engine_revision: Some("0.1.0".into()),
                model_weight_hashes: BTreeMap::from([(
                    "model.ckpt".into(),
                    "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855".into(),
                )]),
                runtime_version: Some("pytorch-2.1.0".into()),
                backend: "Metal".into(),
                device: "mps".into(),
                driver_version: None,
            },
            parameters: GenerationParameters {
                quality: Some("balanced".into()),
                seed: Some(42),
                resolution: Some(256),
                density_threshold: Some(20.0),
                smoothing_iterations: Some(2),
                remove_small_components: Some(true),
                target_face_count: None,
                extra: BTreeMap::new(),
            },
            lineage: AssetLineage {
                parent_asset_id: None,
                parent_asset_version: None,
                operation: AssetOperation::Generate,
                ancestor_asset_ids: Vec::new(),
            },
            geometry: GeometryInventory {
                vertex_count: 12500,
                face_count: 24000,
                component_count: Some(1),
                watertight: Some(true),
                bounding_box: BoundingBox::new([-0.5, -0.5, -0.5], [0.5, 0.5, 0.5]),
                coordinate_system: CoordinateSystem::GltfYUp,
                unit: PhysicalUnit::Meters,
            },
            materials: MaterialInventory {
                channels: vec![ManifestMaterialChannel::VertexColor],
                texture_width: None,
                texture_height: None,
                texture_color_space: None,
                pbr: false,
                unknown_channels: Vec::new(),
            },
            artifacts: vec![OutputArtifact {
                path: "mesh.glb".into(),
                mime_type: "model/gltf-binary".into(),
                byte_length: 1048576,
                sha256: "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855".into(),
            }],
            warnings: Vec::new(),
        }
    }

    #[test]
    fn roundtrip_serialization_preserves_all_fields() {
        let manifest = valid_test_manifest();
        let json_str = manifest.to_json().expect("Serialization failed");
        let parsed = AssetManifest::parse(&json_str).expect("Deserialization failed");
        assert_eq!(manifest, parsed);
    }

    #[test]
    fn schema_version_enforces_version_one() {
        let mut manifest = valid_test_manifest();
        manifest.schema_version = 2;
        let err = manifest.validate().unwrap_err();
        assert!(err.contains("Unsupported manifest schema version"));

        manifest.schema_version = 0;
        let err = manifest.validate().unwrap_err();
        assert!(err.contains("Unsupported manifest schema version"));
    }

    #[test]
    fn bounding_box_validation_rejects_inverted_or_non_finite_bounds() {
        let mut manifest = valid_test_manifest();
        // Inverted X bounds
        manifest.geometry.bounding_box = BoundingBox::new([1.0, -0.5, -0.5], [0.5, 0.5, 0.5]);
        let err = manifest.validate().unwrap_err();
        assert!(err.contains("cannot exceed max[0]"));

        // NaN bounds
        manifest.geometry.bounding_box = BoundingBox::new([f64::NAN, -0.5, -0.5], [0.5, 0.5, 0.5]);
        let err = manifest.validate().unwrap_err();
        assert!(err.contains("finite numbers"));
    }

    #[test]
    fn hash_and_uuid_validation() {
        let mut manifest = valid_test_manifest();
        manifest.asset_id = "not-a-uuid".into();
        assert!(manifest.validate().unwrap_err().contains("Invalid assetId"));

        let mut manifest = valid_test_manifest();
        manifest.artifacts[0].sha256 = "invalid-hash".into();
        assert!(manifest.validate().unwrap_err().contains("invalid SHA-256"));
    }

    #[test]
    fn lineage_requires_parent_asset_id_for_refine() {
        let mut manifest = valid_test_manifest();
        manifest.lineage.operation = AssetOperation::Refine;
        manifest.lineage.parent_asset_id = None;

        let err = manifest.validate().unwrap_err();
        assert!(err.contains("Parent asset ID is required when operation is refine"));

        // With valid parent asset ID
        manifest.lineage.parent_asset_id = Some("550e8400-e29b-41d4-a716-446655440099".into());
        assert!(manifest.validate().is_ok());

        // Self-referencing parent is rejected
        manifest.lineage.parent_asset_id = Some(manifest.asset_id.clone());
        assert!(manifest
            .validate()
            .unwrap_err()
            .contains("Asset cannot be its own parent"));
    }

    #[test]
    fn material_validation_enforces_pbr_truthfulness() {
        let mut manifest = valid_test_manifest();
        // False PBR claim with only vertex_color must fail
        manifest.materials.pbr = true;
        manifest.materials.channels = vec![ManifestMaterialChannel::VertexColor];
        let err = manifest.validate().unwrap_err();
        assert!(err.contains("PBR status cannot be true unless diffuse/color, normal, roughness, and metallic channels are all present"));

        // Missing metallic
        manifest.materials.channels = vec![
            ManifestMaterialChannel::DiffuseTexture,
            ManifestMaterialChannel::Normal,
            ManifestMaterialChannel::Roughness,
        ];
        assert!(manifest.validate().is_err());

        // Full PBR channels present
        manifest.materials.channels = vec![
            ManifestMaterialChannel::DiffuseTexture,
            ManifestMaterialChannel::Normal,
            ManifestMaterialChannel::Roughness,
            ManifestMaterialChannel::Metallic,
        ];
        assert!(manifest.validate().is_ok());

        // Non-PBR with only diffuse texture is completely valid
        manifest.materials.pbr = false;
        manifest.materials.channels = vec![ManifestMaterialChannel::DiffuseTexture];
        assert!(manifest.validate().is_ok());
    }

    #[test]
    fn unknown_material_channels_are_explicitly_flagged_and_preserved() {
        let json_data = r#"{
            "schemaVersion": 1,
            "assetId": "550e8400-e29b-41d4-a716-446655440000",
            "createdAt": "2026-11-24T12:00:00Z",
            "sourceInputs": {},
            "provenance": {
                "engineId": "custom_engine",
                "backend": "CUDA",
                "device": "cuda:0"
            },
            "parameters": {},
            "lineage": {
                "operation": "generate"
            },
            "geometry": {
                "vertexCount": 100,
                "faceCount": 50,
                "boundingBox": { "min": [-1.0, -1.0, -1.0], "max": [1.0, 1.0, 1.0] },
                "coordinateSystem": "glTF Y-up",
                "unit": "meters"
            },
            "materials": {
                "channels": ["diffuse_texture", "specular_tint_experimental"],
                "pbr": false,
                "unknownChannels": ["specular_tint_experimental"]
            },
            "artifacts": [
                {
                    "path": "model.glb",
                    "mimeType": "model/gltf-binary",
                    "byteLength": 5000,
                    "sha256": "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
                }
            ]
        }"#;

        let manifest = AssetManifest::parse(json_data).expect("Should parse with custom channels");
        assert_eq!(manifest.materials.channels.len(), 2);
        assert_eq!(
            manifest.materials.channels[1],
            ManifestMaterialChannel::Other("specular_tint_experimental".into())
        );
        assert_eq!(
            manifest.materials.unknown_channels,
            vec!["specular_tint_experimental".to_string()]
        );
    }

    #[test]
    fn legacy_metrics_migration_produces_valid_manifest() {
        let legacy_json = serde_json::json!({
            "engine": "triposr",
            "device": "mps",
            "quality": "draft",
            "operation": "generate",
            "modelRevision": "0.1.0",
            "torchVersion": "2.1.0",
            "faces": 12000,
            "vertices": 6500,
            "meshQuality": {
                "watertight": true,
                "components": 1
            },
            "textureResolution": "vertex",
            "sourceSha256": "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
            "fileBytes": 512000
        });

        let manifest = AssetManifest::from_legacy_metrics(
            "550e8400-e29b-41d4-a716-446655440000",
            "1732450000000",
            &legacy_json,
            None,
        )
        .expect("Migration failed");

        assert_eq!(manifest.schema_version, 1);
        assert_eq!(manifest.provenance.engine_id, "triposr");
        assert_eq!(manifest.provenance.backend, "Metal");
        assert_eq!(manifest.provenance.device, "mps");
        assert_eq!(manifest.geometry.vertex_count, 6500);
        assert_eq!(manifest.geometry.face_count, 12000);
        assert_eq!(manifest.geometry.watertight, Some(true));
        assert_eq!(manifest.materials.pbr, false);
        assert_eq!(
            manifest.materials.channels,
            vec![ManifestMaterialChannel::VertexColor]
        );
        assert_eq!(manifest.artifacts.len(), 1);
        assert_eq!(manifest.artifacts[0].path, "mesh.glb");
        assert_eq!(manifest.artifacts[0].byte_length, 512000);
        assert!(!manifest.warnings.is_empty());
        assert_eq!(manifest.warnings[0].code, "LEGACY_MIGRATION");
    }

    #[test]
    fn artifact_path_safety_rejects_traversal_or_absolute_paths() {
        let mut manifest = valid_test_manifest();
        manifest.artifacts[0].path = "../outside.glb".into();
        assert!(manifest
            .validate()
            .unwrap_err()
            .contains("must be a safe relative path"));

        manifest.artifacts[0].path = "/etc/passwd".into();
        assert!(manifest
            .validate()
            .unwrap_err()
            .contains("must be a safe relative path"));

        manifest.artifacts[0].path = "C:\\Windows\\System32\\file.glb".into();
        assert!(manifest
            .validate()
            .unwrap_err()
            .contains("must be a safe relative path"));
    }

    #[test]
    fn documentation_schema_example_validates() {
        let doc_example = r#"{
  "schemaVersion": 1,
  "assetId": "e5b8c9a0-1234-4567-89ab-cdef01234567",
  "createdAt": "2026-11-24T12:00:00Z",
  "sourceInputs": {
    "sourceImageId": "3fa85f64-5717-4562-b3fc-2c963f66afa6",
    "sourceImageSha256": "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
    "maskId": "a1b2c3d4-e5f6-7890-abcd-ef1234567890",
    "maskSha256": "4b227777d4dd1fc61c6f884f48641d02b4d121d3fd328cb08b5531fcacdabf8a",
    "originalPrompt": null,
    "composedPrompt": null
  },
  "provenance": {
    "engineId": "triposr",
    "engineRevision": "107cefdc244c39106fa830359024f6a2f1c78871",
    "modelWeightHashes": {
      "model.ckpt": "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
      "config.yaml": "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
    },
    "runtimeVersion": "python-3.11-tsr-v1",
    "backend": "Metal",
    "device": "mps",
    "driverVersion": null
  },
  "parameters": {
    "quality": "balanced",
    "seed": 0,
    "resolution": 128,
    "densityThreshold": 25.0,
    "smoothingIterations": 2,
    "removeSmallComponents": true,
    "targetFaceCount": null,
    "extra": {
      "chunkSize": 4096
    }
  },
  "lineage": {
    "parentAssetId": null,
    "parentAssetVersion": null,
    "operation": "generate",
    "ancestorAssetIds": []
  },
  "geometry": {
    "vertexCount": 24512,
    "faceCount": 48920,
    "componentCount": 1,
    "watertight": false,
    "boundingBox": {
      "min": [-0.52, -0.48, -0.51],
      "max": [0.53, 0.49, 0.52]
    },
    "coordinateSystem": "glTF Y-up",
    "unit": "meters"
  },
  "materials": {
    "channels": [
      "vertex_color",
      "diffuse_texture"
    ],
    "textureWidth": 1024,
    "textureHeight": 1024,
    "textureColorSpace": "sRGB",
    "pbr": false,
    "unknownChannels": []
  },
  "artifacts": [
    {
      "path": "mesh.glb",
      "mimeType": "model/gltf-binary",
      "byteLength": 3456789,
      "sha256": "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08"
    },
    {
      "path": "scene_cache.npz",
      "mimeType": "application/x-numpy",
      "byteLength": 12456789,
      "sha256": "5e884898da28047151d0e56f8dc6292773603d0d6aabbdd62a11ef721d1542d8"
    }
  ],
  "warnings": [
    {
      "code": "NON_MANIFOLD_EDGES",
      "message": "Mesh contains 14 non-manifold edge segments; use Taubin smoothing or slicer repair for 3D printing."
    }
  ]
}"#;

        let parsed = AssetManifest::parse(doc_example).expect("Documented example must be valid");
        assert_eq!(parsed.schema_version, 1);
        assert_eq!(parsed.provenance.engine_id, "triposr");
        assert_eq!(parsed.artifacts.len(), 2);
    }
}
