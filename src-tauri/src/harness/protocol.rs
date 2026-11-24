use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Component, Path, PathBuf};

pub const PROTOCOL_VERSION: u32 = 1;

/// Valid stages during worker inference execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WorkerStage {
    Analyzing,
    Loading,
    Geometry,
    Surface,
    Preparing,
}

impl WorkerStage {
    pub fn as_str(&self) -> &'static str {
        match self {
            WorkerStage::Analyzing => "analyzing",
            WorkerStage::Loading => "loading",
            WorkerStage::Geometry => "geometry",
            WorkerStage::Surface => "surface",
            WorkerStage::Preparing => "preparing",
        }
    }

    pub fn parse_stage(s: &str) -> Result<Self, ProtocolError> {
        match s.to_ascii_lowercase().as_str() {
            "analyzing" => Ok(WorkerStage::Analyzing),
            "loading" => Ok(WorkerStage::Loading),
            "geometry" => Ok(WorkerStage::Geometry),
            "surface" => Ok(WorkerStage::Surface),
            "preparing" => Ok(WorkerStage::Preparing),
            other => Err(ProtocolError::UnknownStage(other.to_string())),
        }
    }
}

/// Structured payload for a successful result manifest.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ResultPayload {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metrics: Option<Value>,

    #[serde(
        default,
        alias = "output_files",
        alias = "files",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub artifacts: Vec<PathBuf>,

    #[serde(
        default,
        alias = "durationMs",
        alias = "durationSeconds",
        skip_serializing_if = "Option::is_none"
    )]
    pub duration_ms: Option<u64>,

    #[serde(
        default,
        alias = "peakMemoryMb",
        alias = "peak_process_memory_mb",
        skip_serializing_if = "Option::is_none"
    )]
    pub peak_memory_mb: Option<f64>,
}

#[allow(dead_code)]
impl ResultPayload {
    pub fn new(metrics: Option<Value>) -> Self {
        Self {
            metrics,
            artifacts: Vec::new(),
            duration_ms: None,
            peak_memory_mb: None,
        }
    }

    pub fn validate_artifacts(
        &self,
        workspace_dir: &Path,
    ) -> Result<Vec<PathBuf>, PathSafetyError> {
        let mut validated = Vec::with_capacity(self.artifacts.len());
        for artifact in &self.artifacts {
            let safe_path = validate_output_path(artifact, workspace_dir)?;
            validated.push(safe_path);
        }
        Ok(validated)
    }
}

/// Standard worker events communicated across IPC.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WorkerEvent {
    #[serde(rename = "accepted")]
    Accepted {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        message: Option<String>,
    },
    #[serde(rename = "stage")]
    Stage {
        stage: WorkerStage,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        message: Option<String>,
    },
    #[serde(rename = "progress")]
    Progress {
        stage: WorkerStage,
        progress: f64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        message: Option<String>,
    },
    #[serde(rename = "warning")]
    Warning {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        code: Option<String>,
        message: String,
    },
    #[serde(rename = "result")]
    Result(ResultPayload),
    #[serde(rename = "error")]
    Error {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        code: Option<String>,
        message: String,
    },
    #[serde(rename = "cancelled")]
    Cancelled {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        message: Option<String>,
    },
}

/// Versioned NDJSON envelope for worker communication.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WorkerEnvelope {
    #[serde(alias = "protocol_version")]
    pub protocol: u32,

    #[serde(alias = "jobId", default, skip_serializing_if = "Option::is_none")]
    pub job_id: Option<String>,

    #[serde(flatten)]
    pub event: WorkerEvent,
}

#[allow(dead_code)]
impl WorkerEnvelope {
    pub fn accepted(job_id: Option<String>, message: Option<String>) -> Self {
        Self {
            protocol: PROTOCOL_VERSION,
            job_id,
            event: WorkerEvent::Accepted { message },
        }
    }

    pub fn stage(job_id: Option<String>, stage: WorkerStage, message: Option<String>) -> Self {
        Self {
            protocol: PROTOCOL_VERSION,
            job_id,
            event: WorkerEvent::Stage { stage, message },
        }
    }

    pub fn progress(
        job_id: Option<String>,
        stage: WorkerStage,
        progress: f64,
        message: Option<String>,
    ) -> Self {
        Self {
            protocol: PROTOCOL_VERSION,
            job_id,
            event: WorkerEvent::Progress {
                stage,
                progress,
                message,
            },
        }
    }

    pub fn warning(job_id: Option<String>, code: Option<String>, message: String) -> Self {
        Self {
            protocol: PROTOCOL_VERSION,
            job_id,
            event: WorkerEvent::Warning { code, message },
        }
    }

    pub fn result(job_id: Option<String>, payload: ResultPayload) -> Self {
        Self {
            protocol: PROTOCOL_VERSION,
            job_id,
            event: WorkerEvent::Result(payload),
        }
    }

    pub fn error(job_id: Option<String>, code: Option<String>, message: String) -> Self {
        Self {
            protocol: PROTOCOL_VERSION,
            job_id,
            event: WorkerEvent::Error { code, message },
        }
    }

    pub fn cancelled(job_id: Option<String>, message: Option<String>) -> Self {
        Self {
            protocol: PROTOCOL_VERSION,
            job_id,
            event: WorkerEvent::Cancelled { message },
        }
    }

    pub fn to_ndjson(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(self)
    }

    /// Parse a single NDJSON line and validate strict protocol conformance.
    pub fn parse(line: &str) -> Result<Self, ProtocolError> {
        let mut val: Value = serde_json::from_str(line.trim())
            .map_err(|e| ProtocolError::MalformedJson(e.to_string()))?;

        let obj = val
            .as_object_mut()
            .ok_or_else(|| ProtocolError::InvalidFormat("Message must be a JSON object".into()))?;

        // 1. Validate protocol version
        if let Some(pv) = obj.remove("protocol_version") {
            if !obj.contains_key("protocol") {
                obj.insert("protocol".to_string(), pv);
            }
        }

        let protocol_val = obj
            .get("protocol")
            .ok_or(ProtocolError::MissingProtocolVersion)?;

        let protocol = protocol_val.as_u64().ok_or_else(|| {
            ProtocolError::InvalidProtocolVersion("Protocol version must be an integer".into())
        })? as u32;

        if protocol != PROTOCOL_VERSION {
            return Err(ProtocolError::UnsupportedProtocolVersion {
                expected: PROTOCOL_VERSION,
                actual: protocol,
            });
        }

        // 2. Normalize jobId -> job_id
        if !obj.contains_key("job_id") {
            if let Some(jid) = obj.remove("jobId") {
                obj.insert("job_id".to_string(), jid);
            }
        }

        // 3. Normalize event -> type
        if !obj.contains_key("type") {
            if let Some(event_kind) = obj.remove("event") {
                obj.insert("type".to_string(), event_kind);
            } else {
                return Err(ProtocolError::MissingEventType);
            }
        }

        let event_type_str = obj
            .get("type")
            .and_then(|v| v.as_str())
            .ok_or(ProtocolError::MissingEventType)?
            .to_string();

        // 4. Validate event-specific constraints before typed deserialization
        match event_type_str.as_str() {
            "accepted" => {}
            "stage" => {
                let stage_str = obj
                    .get("stage")
                    .and_then(|v| v.as_str())
                    .ok_or(ProtocolError::MissingField("stage"))?;
                WorkerStage::parse_stage(stage_str)?;
            }
            "progress" => {
                let stage_str = obj
                    .get("stage")
                    .and_then(|v| v.as_str())
                    .ok_or(ProtocolError::MissingField("stage"))?;
                WorkerStage::parse_stage(stage_str)?;

                let prog_val = obj
                    .get("progress")
                    .ok_or(ProtocolError::MissingField("progress"))?;
                let prog = prog_val.as_f64().ok_or_else(|| {
                    ProtocolError::InvalidField("progress must be a number".into())
                })?;
                if !prog.is_finite() {
                    return Err(ProtocolError::InvalidField(
                        "progress must be finite".into(),
                    ));
                }
            }
            "warning" => {
                if !obj.contains_key("message")
                    || obj.get("message").and_then(|v| v.as_str()).is_none()
                {
                    return Err(ProtocolError::MissingField("message"));
                }
            }
            "result" => {
                // Check if result has metrics, artifacts, or duration
            }
            "error" => {
                if !obj.contains_key("message")
                    || obj.get("message").and_then(|v| v.as_str()).is_none()
                {
                    return Err(ProtocolError::MissingField("message"));
                }
            }
            "cancelled" => {}
            other => return Err(ProtocolError::UnknownEventType(other.to_string())),
        }

        // 5. Typed deserialization
        let envelope: WorkerEnvelope =
            serde_json::from_value(val).map_err(|e| ProtocolError::InvalidFormat(e.to_string()))?;

        Ok(envelope)
    }

    /// Validate any output artifact paths reported in the result event.
    pub fn validate_output_paths(&self, workspace_dir: &Path) -> Result<(), ProtocolError> {
        if let WorkerEvent::Result(ref res) = self.event {
            res.validate_artifacts(workspace_dir)
                .map_err(ProtocolError::PathSafety)?;

            // If metrics contains explicit outputPath, validate it too
            if let Some(ref m) = res.metrics {
                if let Some(output_str) = m.get("outputPath").and_then(|v| v.as_str()) {
                    validate_output_path(Path::new(output_str), workspace_dir)
                        .map_err(ProtocolError::PathSafety)?;
                }
            }
        }
        Ok(())
    }
}

/// Path safety validation errors.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathSafetyError {
    ParentTraversal(PathBuf),
    OutsideWorkspace {
        path: PathBuf,
        workspace: PathBuf,
    },
    SymlinkEscape {
        path: PathBuf,
        target: PathBuf,
        workspace: PathBuf,
    },
    InvalidPath(String),
}

impl std::fmt::Display for PathSafetyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PathSafetyError::ParentTraversal(p) => {
                write!(f, "Path traversal forbidden: '{}'", p.display())
            }
            PathSafetyError::OutsideWorkspace { path, workspace } => {
                write!(
                    f,
                    "Path '{}' is outside the job workspace '{}'",
                    path.display(),
                    workspace.display()
                )
            }
            PathSafetyError::SymlinkEscape {
                path,
                target,
                workspace,
            } => {
                write!(
                    f,
                    "Symlink '{}' resolves to '{}' which escapes workspace '{}'",
                    path.display(),
                    target.display(),
                    workspace.display()
                )
            }
            PathSafetyError::InvalidPath(msg) => write!(f, "Invalid path: {msg}"),
        }
    }
}

impl std::error::Error for PathSafetyError {}

/// Strips Windows extended-length prefix (\\?\) for consistent path comparisons.
fn strip_verbatim_prefix(path: &Path) -> PathBuf {
    let s = path.to_string_lossy();
    if let Some(stripped) = s.strip_prefix(r"\\?\") {
        PathBuf::from(stripped)
    } else {
        path.to_path_buf()
    }
}

/// Strictly validates that an output artifact candidate path resides within `workspace_dir`.
/// Rejects path traversal (e.g. `..`), absolute paths outside workspace, and symlinks pointing outside.
pub fn validate_output_path(
    candidate: &Path,
    workspace_dir: &Path,
) -> Result<PathBuf, PathSafetyError> {
    // 1. Check for any parent traversal components ('..')
    for comp in candidate.components() {
        if matches!(comp, Component::ParentDir) {
            return Err(PathSafetyError::ParentTraversal(candidate.to_path_buf()));
        }
    }

    // 2. Canonicalize workspace if it exists
    let canonical_workspace = if workspace_dir.exists() {
        strip_verbatim_prefix(&workspace_dir.canonicalize().map_err(|e| {
            PathSafetyError::InvalidPath(format!("Failed to canonicalize workspace: {e}"))
        })?)
    } else {
        strip_verbatim_prefix(workspace_dir)
    };

    // 3. Resolve target path
    let target = if candidate.is_absolute() {
        candidate.to_path_buf()
    } else {
        workspace_dir.join(candidate)
    };

    // 4. Verify containment
    let normalized_target = strip_verbatim_prefix(&target);

    // If target exists on disk, canonicalize resolves symlinks
    if target.exists() {
        let canonical_target = strip_verbatim_prefix(&target.canonicalize().map_err(|e| {
            PathSafetyError::InvalidPath(format!("Failed to canonicalize target path: {e}"))
        })?);

        if !canonical_target.starts_with(&canonical_workspace) {
            if normalized_target.starts_with(&canonical_workspace) {
                return Err(PathSafetyError::SymlinkEscape {
                    path: candidate.to_path_buf(),
                    target: canonical_target,
                    workspace: canonical_workspace,
                });
            } else {
                return Err(PathSafetyError::OutsideWorkspace {
                    path: candidate.to_path_buf(),
                    workspace: canonical_workspace,
                });
            }
        }
        return Ok(canonical_target);
    }

    // Target does not yet exist: inspect ancestors to verify no symlink breakout in existing prefix
    let mut curr = target.as_path();
    while let Some(parent) = curr.parent() {
        if parent.exists() {
            let canonical_parent = strip_verbatim_prefix(&parent.canonicalize().map_err(|e| {
                PathSafetyError::InvalidPath(format!("Failed to canonicalize parent: {e}"))
            })?);
            if !canonical_parent.starts_with(&canonical_workspace) {
                return Err(PathSafetyError::OutsideWorkspace {
                    path: candidate.to_path_buf(),
                    workspace: canonical_workspace,
                });
            }
            break;
        }
        curr = parent;
    }

    if !normalized_target.starts_with(&canonical_workspace) {
        return Err(PathSafetyError::OutsideWorkspace {
            path: candidate.to_path_buf(),
            workspace: canonical_workspace,
        });
    }

    Ok(normalized_target)
}

/// Errors raised during worker protocol deserialization and validation.
#[derive(Debug, Clone, PartialEq)]
pub enum ProtocolError {
    MalformedJson(String),
    InvalidFormat(String),
    MissingProtocolVersion,
    InvalidProtocolVersion(String),
    UnsupportedProtocolVersion { expected: u32, actual: u32 },
    MissingEventType,
    UnknownEventType(String),
    MissingField(&'static str),
    InvalidField(String),
    UnknownStage(String),
    PathSafety(PathSafetyError),
}

impl std::fmt::Display for ProtocolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProtocolError::MalformedJson(e) => write!(f, "Malformed protocol message JSON: {e}"),
            ProtocolError::InvalidFormat(e) => write!(f, "Invalid protocol envelope format: {e}"),
            ProtocolError::MissingProtocolVersion => {
                write!(f, "Missing protocol version in worker message")
            }
            ProtocolError::InvalidProtocolVersion(e) => write!(f, "Invalid protocol version: {e}"),
            ProtocolError::UnsupportedProtocolVersion { expected, actual } => {
                write!(
                    f,
                    "Unsupported worker protocol version: {actual} (expected {expected})"
                )
            }
            ProtocolError::MissingEventType => write!(f, "Missing event type in worker message"),
            ProtocolError::UnknownEventType(t) => write!(f, "Unknown worker event type: '{t}'"),
            ProtocolError::MissingField(fld) => write!(f, "Missing required field: '{fld}'"),
            ProtocolError::InvalidField(msg) => write!(f, "Invalid field in worker message: {msg}"),
            ProtocolError::UnknownStage(s) => write!(f, "Unknown worker stage: '{s}'"),
            ProtocolError::PathSafety(err) => write!(f, "Worker path safety error: {err}"),
        }
    }
}

impl std::error::Error for ProtocolError {}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn parse_all_valid_event_types() {
        let job = Some("job-test-42".to_string());

        // 1. accepted
        let json =
            r#"{"protocol":1,"job_id":"job-test-42","type":"accepted","message":"Job accepted"}"#;
        let env = WorkerEnvelope::parse(json).expect("accepted should parse");
        assert_eq!(env.protocol, 1);
        assert_eq!(env.job_id, job);
        assert_eq!(
            env.event,
            WorkerEvent::Accepted {
                message: Some("Job accepted".into())
            }
        );

        // 2. stage
        let json = r#"{"protocol":1,"job_id":"job-test-42","type":"stage","stage":"analyzing","message":"Analyzing image"}"#;
        let env = WorkerEnvelope::parse(json).expect("stage should parse");
        assert_eq!(
            env.event,
            WorkerEvent::Stage {
                stage: WorkerStage::Analyzing,
                message: Some("Analyzing image".into()),
            }
        );

        // 3. progress
        let json = r#"{"protocol":1,"job_id":"job-test-42","type":"progress","stage":"geometry","progress":45.0,"message":"Reconstructing"}"#;
        let env = WorkerEnvelope::parse(json).expect("progress should parse");
        assert_eq!(
            env.event,
            WorkerEvent::Progress {
                stage: WorkerStage::Geometry,
                progress: 45.0,
                message: Some("Reconstructing".into()),
            }
        );

        // 4. warning
        let json = r#"{"protocol":1,"job_id":"job-test-42","type":"warning","code":"HIGH_MEMORY","message":"Approaching RAM limit"}"#;
        let env = WorkerEnvelope::parse(json).expect("warning should parse");
        assert_eq!(
            env.event,
            WorkerEvent::Warning {
                code: Some("HIGH_MEMORY".into()),
                message: "Approaching RAM limit".into(),
            }
        );

        // 5. result
        let json = r#"{"protocol":1,"job_id":"job-test-42","type":"result","metrics":{"faces":12000},"artifacts":["mesh.glb"],"duration_ms":3400,"peak_memory_mb":512.5}"#;
        let env = WorkerEnvelope::parse(json).expect("result should parse");
        match env.event {
            WorkerEvent::Result(payload) => {
                assert_eq!(payload.artifacts, vec![PathBuf::from("mesh.glb")]);
                assert_eq!(payload.duration_ms, Some(3400));
                assert_eq!(payload.peak_memory_mb, Some(512.5));
                assert!(payload.metrics.is_some());
            }
            _ => panic!("Expected WorkerEvent::Result"),
        }

        // 6. error
        let json = r#"{"protocol":1,"job_id":"job-test-42","type":"error","code":"OOM","message":"CUDA out of memory"}"#;
        let env = WorkerEnvelope::parse(json).expect("error should parse");
        assert_eq!(
            env.event,
            WorkerEvent::Error {
                code: Some("OOM".into()),
                message: "CUDA out of memory".into(),
            }
        );

        // 7. cancelled
        let json = r#"{"protocol":1,"job_id":"job-test-42","type":"cancelled","message":"User cancelled"}"#;
        let env = WorkerEnvelope::parse(json).expect("cancelled should parse");
        assert_eq!(
            env.event,
            WorkerEvent::Cancelled {
                message: Some("User cancelled".into()),
            }
        );
    }

    #[test]
    fn alias_support() {
        // protocol_version alias, event alias, jobId alias
        let json = r#"{"protocol_version":1,"jobId":"job-99","event":"stage","stage":"surface"}"#;
        let env = WorkerEnvelope::parse(json).expect("aliases should parse");
        assert_eq!(env.protocol, 1);
        assert_eq!(env.job_id.as_deref(), Some("job-99"));
        assert_eq!(
            env.event,
            WorkerEvent::Stage {
                stage: WorkerStage::Surface,
                message: None,
            }
        );
    }

    #[test]
    fn reject_malformed_json_and_non_objects() {
        assert!(matches!(
            WorkerEnvelope::parse("{not json"),
            Err(ProtocolError::MalformedJson(_))
        ));
        assert!(matches!(
            WorkerEnvelope::parse("[1, 2, 3]"),
            Err(ProtocolError::InvalidFormat(_))
        ));
        assert!(matches!(
            WorkerEnvelope::parse("\"string message\""),
            Err(ProtocolError::InvalidFormat(_))
        ));
    }

    #[test]
    fn reject_missing_protocol_or_unknown_version() {
        // Missing protocol
        let json = r#"{"type":"progress","stage":"analyzing","progress":10.0}"#;
        assert_eq!(
            WorkerEnvelope::parse(json),
            Err(ProtocolError::MissingProtocolVersion)
        );

        // Unknown protocol version 2
        let json = r#"{"protocol":2,"type":"progress","stage":"analyzing","progress":10.0}"#;
        assert_eq!(
            WorkerEnvelope::parse(json),
            Err(ProtocolError::UnsupportedProtocolVersion {
                expected: 1,
                actual: 2,
            })
        );

        // Protocol version 0
        let json = r#"{"protocol":0,"type":"progress","stage":"analyzing","progress":10.0}"#;
        assert_eq!(
            WorkerEnvelope::parse(json),
            Err(ProtocolError::UnsupportedProtocolVersion {
                expected: 1,
                actual: 0,
            })
        );
    }

    #[test]
    fn reject_missing_or_unknown_event_type() {
        let json = r#"{"protocol":1,"job_id":"job-1"}"#;
        assert_eq!(
            WorkerEnvelope::parse(json),
            Err(ProtocolError::MissingEventType)
        );

        let json = r#"{"protocol":1,"type":"unknown_future_event"}"#;
        assert_eq!(
            WorkerEnvelope::parse(json),
            Err(ProtocolError::UnknownEventType(
                "unknown_future_event".into()
            ))
        );
    }

    #[test]
    fn reject_invalid_stages_and_progress() {
        // Unknown stage
        let json = r#"{"protocol":1,"type":"stage","stage":"baking"}"#;
        assert_eq!(
            WorkerEnvelope::parse(json),
            Err(ProtocolError::UnknownStage("baking".into()))
        );

        // Missing stage in progress
        let json = r#"{"protocol":1,"type":"progress","progress":50.0}"#;
        assert_eq!(
            WorkerEnvelope::parse(json),
            Err(ProtocolError::MissingField("stage"))
        );

        // Missing progress in progress
        let json = r#"{"protocol":1,"type":"progress","stage":"geometry"}"#;
        assert_eq!(
            WorkerEnvelope::parse(json),
            Err(ProtocolError::MissingField("progress"))
        );

        // Non-finite progress
        let json =
            r#"{"protocol":1,"type":"progress","stage":"geometry","progress":"not a number"}"#;
        assert!(matches!(
            WorkerEnvelope::parse(json),
            Err(ProtocolError::InvalidField(_))
        ));
    }

    #[test]
    fn roundtrip_serialization() {
        let original = WorkerEnvelope::progress(
            Some("job-roundtrip".into()),
            WorkerStage::Preparing,
            92.5,
            Some("Saving GLB".into()),
        );
        let ndjson = original.to_ndjson().expect("Serialization failed");
        let parsed = WorkerEnvelope::parse(&ndjson).expect("Deserialization failed");
        assert_eq!(original, parsed);

        let result_orig = WorkerEnvelope::result(
            Some("job-res".into()),
            ResultPayload {
                metrics: Some(serde_json::json!({"triangles": 5000})),
                artifacts: vec![PathBuf::from("mesh.glb"), PathBuf::from("preview.png")],
                duration_ms: Some(1234),
                peak_memory_mb: Some(256.0),
            },
        );
        let ndjson2 = result_orig.to_ndjson().expect("Serialization failed");
        let parsed2 = WorkerEnvelope::parse(&ndjson2).expect("Deserialization failed");
        assert_eq!(result_orig, parsed2);
    }

    #[test]
    fn path_safety_validation() {
        let temp_dir =
            std::env::temp_dir().join(format!("sculpt-test-safe-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&temp_dir).unwrap();

        let sub_dir = temp_dir.join("sub");
        fs::create_dir_all(&sub_dir).unwrap();

        // 1. Safe relative paths
        let safe_rel = Path::new("mesh.glb");
        let validated =
            validate_output_path(safe_rel, &temp_dir).expect("safe relative path should succeed");
        assert!(validated.starts_with(strip_verbatim_prefix(&temp_dir.canonicalize().unwrap())));

        let safe_sub = Path::new("sub/texture.png");
        let validated_sub =
            validate_output_path(safe_sub, &temp_dir).expect("safe subpath should succeed");
        assert!(validated_sub.starts_with(strip_verbatim_prefix(&temp_dir.canonicalize().unwrap())));

        // 2. Safe absolute path
        let safe_abs = temp_dir.join("mesh.glb");
        fs::write(&safe_abs, b"test").unwrap();
        let validated_abs =
            validate_output_path(&safe_abs, &temp_dir).expect("safe absolute path should succeed");
        assert_eq!(
            validated_abs,
            strip_verbatim_prefix(&safe_abs.canonicalize().unwrap())
        );

        // 3. Parent traversal ('..')
        let traversal_rel = Path::new("../../etc/passwd");
        assert!(matches!(
            validate_output_path(traversal_rel, &temp_dir),
            Err(PathSafetyError::ParentTraversal(_))
        ));

        let traversal_nested = Path::new("sub/../../escaped.txt");
        assert!(matches!(
            validate_output_path(traversal_nested, &temp_dir),
            Err(PathSafetyError::ParentTraversal(_))
        ));

        // 4. Absolute paths outside workspace
        let outside_dir =
            std::env::temp_dir().join(format!("sculpt-outside-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&outside_dir).unwrap();
        let outside_file = outside_dir.join("secret.txt");
        fs::write(&outside_file, b"secret").unwrap();

        assert!(matches!(
            validate_output_path(&outside_file, &temp_dir),
            Err(PathSafetyError::OutsideWorkspace { .. })
        ));

        // 5. Envelope result validation
        let mut unsafe_res = ResultPayload::new(None);
        unsafe_res.artifacts = vec![PathBuf::from("../../outside.glb")];
        let env = WorkerEnvelope::result(Some("job-1".into()), unsafe_res);

        assert!(matches!(
            env.validate_output_paths(&temp_dir),
            Err(ProtocolError::PathSafety(PathSafetyError::ParentTraversal(
                _
            )))
        ));

        // Cleanup
        let _ = fs::remove_dir_all(&temp_dir);
        let _ = fs::remove_dir_all(&outside_dir);
    }
}
