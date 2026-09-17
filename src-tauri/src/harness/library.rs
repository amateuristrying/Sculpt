//! Durable, local ownership of imported sources, jobs and access state.
//!
//! Each mutation writes a complete versioned snapshot before changing memory.
//! Paths are derived from canonical UUIDs, never accepted from saved JSON. The
//! lifetime file lock also prevents two app processes from racing trial usage.
use super::{
    assets::{self, SourceAsset, SourceRecord},
    runtime::{GeneratedAsset, GenerationRequest},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
#[cfg(unix)]
use std::os::{fd::AsRawFd, unix::fs::OpenOptionsExt};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

const VERSION: u32 = 1;
const MAX_SNAPSHOT_BYTES: u64 = 32 * 1024 * 1024;
const MAX_RECORDS: usize = 20_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum JobState {
    Running,
    Succeeded,
    Failed,
    Cancelled,
    Interrupted,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct JobRecord {
    pub id: String,
    pub request: GenerationRequest,
    pub state: JobState,
    pub created_at: String,
    pub updated_at: String,
    pub error: Option<String>,
    pub asset: Option<GeneratedAsset>,
    pub artifact_sha256: Option<String>,
    pub artifact_byte_length: Option<u64>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SavedSource {
    asset: SourceAsset,
    extension: String,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Snapshot {
    version: u32,
    sources: BTreeMap<String, SavedSource>,
    jobs: BTreeMap<String, JobRecord>,
    signed_license: Option<String>,
    trial_success_job: Option<String>,
}

impl Default for Snapshot {
    fn default() -> Self {
        Self {
            version: VERSION,
            sources: BTreeMap::new(),
            jobs: BTreeMap::new(),
            signed_license: None,
            trial_success_job: None,
        }
    }
}

pub struct Library {
    root: PathBuf,
    snapshot: Snapshot,
    // Keep the open descriptor, not just its path: flock is released on close.
    _lock: File,
}

impl Library {
    pub fn open(root: PathBuf) -> Result<Self, String> {
        fs::create_dir_all(&root).map_err(storage_error)?;
        reject_symlink(&root)?;
        let root = root.canonicalize().map_err(storage_error)?;
        let lock = open_private(&root.join(".library.lock"), true)?;
        #[cfg(unix)]
        if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
            return Err("The Sculpt library is open in another app process. Close that process and try again.".into());
        }
        #[cfg(not(unix))]
        return Err("Durable library locking is not implemented on this operating system".into());

        ensure_directory(&root, "sources")?;
        ensure_directory(&root, "jobs")?;
        let path = root.join("library.json");
        let exists = path.try_exists().map_err(storage_error)?;
        // A dangling symlink must also fail instead of being treated as a new trial.
        if fs::symlink_metadata(&path).is_ok() {
            reject_symlink(&path)?;
        }
        let snapshot = if exists {
            let mut file = open_private(&path, false)?;
            if file.metadata().map_err(storage_error)?.len() > MAX_SNAPSHOT_BYTES {
                return Err("The Sculpt library exceeds its safe size limit".into());
            }
            let mut bytes = Vec::new();
            Read::by_ref(&mut file)
                .take(MAX_SNAPSHOT_BYTES + 1)
                .read_to_end(&mut bytes)
                .map_err(storage_error)?;
            if bytes.len() as u64 > MAX_SNAPSHOT_BYTES {
                return Err("The Sculpt library exceeds its safe size limit".into());
            }
            serde_json::from_slice::<Snapshot>(&bytes)
                .map_err(|_| "The Sculpt library is damaged. Restore library.json from a backup; it has not been reset.".to_string())?
        } else {
            Snapshot::default()
        };
        validate_snapshot(&snapshot)?;
        let mut library = Self {
            root,
            snapshot,
            _lock: lock,
        };
        let mut recovered = library.snapshot.clone();
        let mut changed = !exists;
        for job in recovered.jobs.values_mut() {
            if job.state == JobState::Running {
                job.state = JobState::Interrupted;
                job.updated_at = now();
                job.error = Some(
                    "Sculpt closed before this generation completed. Generate again to retry."
                        .into(),
                );
                changed = true;
            }
        }
        if changed {
            library.commit(recovered)?;
        }
        Ok(library)
    }

    pub fn source_directory(&self) -> Result<PathBuf, String> {
        ensure_directory(&self.root, "sources")
    }

    pub fn job_directory(&self, id: &str) -> Result<PathBuf, String> {
        validate_id(id)?;
        let jobs = ensure_directory(&self.root, "jobs")?;
        let path = jobs.join(id);
        if fs::symlink_metadata(&path).is_ok() {
            reject_symlink(&path)?;
            if !path.is_dir() {
                return Err("The job location is not a directory".into());
            }
        }
        Ok(path)
    }

    pub fn register_source(&mut self, record: SourceRecord) -> Result<(), String> {
        validate_source_asset(&record.asset)?;
        let extension = extension(&record.asset.mime_type)?;
        let expected = self
            .source_directory()?
            .join(format!("{}.{}", record.asset.id, extension));
        if record.path != expected {
            return Err("The source image is outside the Sculpt library".into());
        }
        verify_source_file(&record.path, &record.asset.sha256)?;
        if self.snapshot.sources.contains_key(&record.asset.id) {
            return Err("Source identifier already exists".into());
        }
        let mut next = self.snapshot.clone();
        next.sources.insert(
            record.asset.id.clone(),
            SavedSource {
                asset: record.asset,
                extension: extension.into(),
            },
        );
        self.commit(next)
    }

    pub fn source(&self, id: &str) -> Result<SourceRecord, String> {
        validate_id(id)?;
        let saved = self
            .snapshot
            .sources
            .get(id)
            .ok_or("Unknown source image; import it again")?;
        let path = self
            .source_directory()?
            .join(format!("{id}.{}", saved.extension));
        verify_source_file(&path, &saved.asset.sha256)?;
        Ok(SourceRecord {
            asset: saved.asset.clone(),
            path,
        })
    }

    /// Return the same bounded, hash-verified bytes that will cross the IPC
    /// boundary; never follow a fresh file path with an unbounded read.
    pub fn read_source(&self, id: &str) -> Result<(SourceAsset, Vec<u8>), String> {
        let source = self.source(id)?;
        let file = open_private(&source.path, false)?;
        let mut bytes = Vec::new();
        file.take(assets::MAX_IMAGE_BYTES as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(storage_error)?;
        if bytes.len() > assets::MAX_IMAGE_BYTES
            || format!("{:x}", Sha256::digest(&bytes)) != source.asset.sha256
        {
            return Err("The stored source image changed or is damaged; import it again".into());
        }
        Ok((source.asset, bytes))
    }

    pub fn begin_job(&mut self, id: &str, request: GenerationRequest) -> Result<(), String> {
        validate_id(id)?;
        validate_request(&request)?;
        if self.snapshot.jobs.contains_key(id) {
            return Err("Job identifier already exists; use a new identifier to retry".into());
        }
        if self
            .snapshot
            .jobs
            .values()
            .any(|job| job.state == JobState::Running)
        {
            return Err("A generation is already running".into());
        }
        if let Some(source_id) = &request.source_id {
            self.source(source_id)?;
        } else if request.engine_id != "demo" {
            return Err("Import an image before generating".into());
        }
        validate_parent(&self.snapshot, id, &request)?;
        if let Some(parent) = &request.parent_asset_id {
            // Refinement is only admitted for a real, intact asset owned by
            // this library. A frontend flag cannot turn a new image into a
            // free refinement of some unrelated reconstruction.
            self.asset_path(parent)?;
        }
        let directory = self.job_directory(id)?;
        if directory.try_exists().map_err(storage_error)? {
            return Err("Job output directory already exists; use a new identifier".into());
        }
        let timestamp = now();
        let mut next = self.snapshot.clone();
        next.jobs.insert(
            id.into(),
            JobRecord {
                id: id.into(),
                request,
                state: JobState::Running,
                created_at: timestamp.clone(),
                updated_at: timestamp,
                error: None,
                asset: None,
                artifact_sha256: None,
                artifact_byte_length: None,
            },
        );
        self.commit(next)
    }

    /// The returned record is authoritative: cancellation wins if it arrived
    /// before this transaction, and never consumes the successful-use trial.
    pub fn finish_job(
        &mut self,
        id: &str,
        result: Result<GeneratedAsset, String>,
        cancelled: bool,
    ) -> Result<JobRecord, String> {
        let old = self.snapshot.jobs.get(id).ok_or("Unknown generation job")?;
        let (state, asset, error) = if cancelled {
            (
                JobState::Cancelled,
                None,
                Some("Generation cancelled".into()),
            )
        } else {
            match result {
                Ok(asset) => {
                    if asset.id != id {
                        return Err("Generated asset identity does not match its job".into());
                    }
                    if asset.simulated != (old.request.engine_id == "demo") {
                        return Err("The runtime returned an unexpected asset type".into());
                    }
                    (JobState::Succeeded, Some(asset), None)
                }
                Err(message) => (
                    JobState::Failed,
                    None,
                    Some(message.chars().take(8192).collect()),
                ),
            }
        };
        if old.state != JobState::Running {
            if old.state == state
                && old.error == error
                && serde_json::to_value(&old.asset).map_err(storage_error)?
                    == serde_json::to_value(&asset).map_err(storage_error)?
            {
                return Ok(old.clone());
            }
            return Err("A completed generation cannot change state".into());
        }
        let artifact = if asset.as_ref().is_some_and(|asset| !asset.simulated) {
            let source_id = old
                .request
                .source_id
                .as_deref()
                .ok_or("The job has no source image")?;
            self.source(source_id)?;
            let path = self.mesh_path(id)?;
            let bytes = assets::read_valid_glb(&path)?;
            open_private(&path, false)?
                .sync_all()
                .map_err(storage_error)?;
            sync_directory(path.parent().ok_or("Invalid asset directory")?)?;
            Some((format!("{:x}", Sha256::digest(&bytes)), bytes.len() as u64))
        } else {
            None
        };
        let mut next = self.snapshot.clone();
        let job = next.jobs.get_mut(id).ok_or("Unknown generation job")?;
        job.state = state;
        job.asset = asset;
        job.error = error;
        job.artifact_sha256 = artifact.as_ref().map(|(hash, _)| hash.clone());
        job.artifact_byte_length = artifact.map(|(_, length)| length);
        job.updated_at = now();
        let completed = job.clone();
        if completed
            .asset
            .as_ref()
            .is_some_and(|asset| !asset.simulated)
            && completed.request.parent_asset_id.is_none()
            && next.trial_success_job.is_none()
        {
            next.trial_success_job = Some(id.into());
        }
        self.commit(next)?;
        Ok(completed)
    }

    pub fn list_jobs(&self, limit: usize) -> Vec<JobRecord> {
        let mut jobs: Vec<_> = self.snapshot.jobs.values().cloned().collect();
        jobs.sort_by(|a, b| {
            b.created_at
                .parse::<u128>()
                .unwrap_or_default()
                .cmp(&a.created_at.parse::<u128>().unwrap_or_default())
                .then_with(|| b.id.cmp(&a.id))
        });
        jobs.truncate(limit.min(500));
        jobs
    }

    pub fn job(&self, id: &str) -> Option<JobRecord> {
        self.snapshot.jobs.get(id).cloned()
    }

    pub fn asset_path(&self, id: &str) -> Result<PathBuf, String> {
        let job = self
            .snapshot
            .jobs
            .get(id)
            .ok_or("Unknown generated asset")?;
        if job.state != JobState::Succeeded
            || !job.asset.as_ref().is_some_and(|asset| !asset.simulated)
        {
            return Err("This job does not have a completed 3D asset".into());
        }
        // The finished mesh remains useful if its original image later goes
        // missing. Source integrity was established at successful completion.
        let path = self.mesh_path(id)?;
        let mut file = open_private(&path, false)?;
        let expected_length = job
            .artifact_byte_length
            .ok_or("The saved asset has no integrity record")?;
        if file.metadata().map_err(storage_error)?.len() != expected_length {
            return Err("The saved 3D asset changed or is damaged".into());
        }
        let mut hash = Sha256::new();
        let mut buffer = [0u8; 64 * 1024];
        let mut total = 0u64;
        loop {
            let count = file.read(&mut buffer).map_err(storage_error)?;
            if count == 0 {
                break;
            }
            total += count as u64;
            if total > assets::MAX_GLB_BYTES as u64 {
                return Err("The saved asset exceeds its size limit".into());
            }
            hash.update(&buffer[..count]);
        }
        if total != expected_length
            || Some(format!("{:x}", hash.finalize())).as_deref() != job.artifact_sha256.as_deref()
        {
            return Err("The saved 3D asset changed or is damaged".into());
        }
        Ok(path)
    }

    fn mesh_path(&self, id: &str) -> Result<PathBuf, String> {
        let path = self.job_directory(id)?.join("mesh.glb");
        reject_symlink(&path)?;
        if !path.is_file() {
            return Err("The generated asset is missing".into());
        }
        Ok(path)
    }

    pub fn signed_license(&self) -> Option<&str> {
        self.snapshot.signed_license.as_deref()
    }

    pub fn set_signed_license(&mut self, license: Option<String>) -> Result<(), String> {
        if license
            .as_ref()
            .is_some_and(|license| license.len() > 16_384 || license.is_empty())
        {
            return Err("Invalid license size".into());
        }
        let mut next = self.snapshot.clone();
        next.signed_license = license;
        self.commit(next)
    }

    pub fn trial_success_job(&self) -> Option<&str> {
        self.snapshot.trial_success_job.as_deref()
    }

    fn commit(&mut self, next: Snapshot) -> Result<(), String> {
        validate_snapshot(&next)?;
        let bytes = serde_json::to_vec(&next).map_err(storage_error)?;
        if bytes.len() as u64 > MAX_SNAPSHOT_BYTES {
            return Err("The Sculpt library exceeds its safe size limit".into());
        }
        let destination = self.root.join("library.json");
        if fs::symlink_metadata(&destination).is_ok() {
            reject_symlink(&destination)?;
            if !destination.is_file() {
                return Err("The Sculpt library location is not a regular file".into());
            }
        }
        let temporary = self
            .root
            .join(format!(".library-{}.tmp", uuid::Uuid::new_v4()));
        let result = (|| {
            let mut options = OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
            let mut file = options.open(&temporary).map_err(storage_error)?;
            file.write_all(&bytes).map_err(storage_error)?;
            file.sync_all().map_err(storage_error)?;
            // Opening/syncing the directory before rename catches inaccessible or
            // unsupported storage before the commit point.
            let directory = File::open(&self.root).map_err(storage_error)?;
            directory.sync_all().map_err(storage_error)?;
            fs::rename(&temporary, &destination).map_err(storage_error)?;
            // Rename is the commit point. Keep memory aligned even if a filesystem
            // cannot confirm the final directory flush after accepting the rename.
            self.snapshot = next;
            directory.sync_all().map_err(|e| {
                format!("The library was saved, but its durability could not be confirmed: {e}")
            })
        })();
        if temporary.exists() {
            let _ = fs::remove_file(&temporary);
        }
        result
    }
}

fn validate_snapshot(snapshot: &Snapshot) -> Result<(), String> {
    if snapshot.version != VERSION {
        return Err(
            "This Sculpt library version is unsupported; update Sculpt before opening it".into(),
        );
    }
    if snapshot.sources.len() > MAX_RECORDS || snapshot.jobs.len() > MAX_RECORDS {
        return Err("The Sculpt library exceeds its record limit".into());
    }
    if snapshot
        .signed_license
        .as_ref()
        .is_some_and(|license| license.is_empty() || license.len() > 16_384)
    {
        return Err("The saved license has an invalid size".into());
    }
    for (id, source) in &snapshot.sources {
        validate_source_asset(&source.asset)?;
        if id != &source.asset.id || source.extension != extension(&source.asset.mime_type)? {
            return Err("The saved source image identity is invalid".into());
        }
    }
    for (id, job) in &snapshot.jobs {
        validate_id(id)?;
        validate_request(&job.request)?;
        validate_parent(snapshot, id, &job.request)?;
        if id != &job.id
            || job.created_at.parse::<u128>().is_err()
            || job.updated_at.parse::<u128>().is_err()
        {
            return Err("The saved generation record is invalid".into());
        }
        if let Some(source_id) = &job.request.source_id {
            if !snapshot.sources.contains_key(source_id) {
                return Err("A saved generation refers to an unknown source".into());
            }
        } else if job.request.engine_id != "demo" {
            return Err("A saved reconstruction has no source image".into());
        }
        if job.error.as_ref().is_some_and(|error| error.len() > 32_768) {
            return Err("A saved generation error exceeds its size limit".into());
        }
        match job.state {
            JobState::Succeeded => {
                let asset = job
                    .asset
                    .as_ref()
                    .ok_or("A completed job is missing its asset record")?;
                if asset.id != *id
                    || asset.simulated != (job.request.engine_id == "demo")
                    || asset.generated_at.parse::<u128>().is_err()
                    || job.error.is_some()
                {
                    return Err("A completed job contains an invalid asset record".into());
                }
                if !asset.simulated {
                    if !job.artifact_sha256.as_deref().is_some_and(valid_sha)
                        || !job.artifact_byte_length.is_some_and(|length| {
                            (20..=assets::MAX_GLB_BYTES as u64).contains(&length)
                        })
                    {
                        return Err(
                            "A completed reconstruction has no valid integrity record".into()
                        );
                    }
                } else if job.artifact_sha256.is_some() || job.artifact_byte_length.is_some() {
                    return Err("A simulated asset cannot have an artifact integrity record".into());
                }
            }
            JobState::Running
                if job.asset.is_none()
                    && job.error.is_none()
                    && job.artifact_sha256.is_none()
                    && job.artifact_byte_length.is_none() =>
            {
                ()
            }
            JobState::Failed | JobState::Cancelled | JobState::Interrupted
                if job.asset.is_none()
                    && job.error.is_some()
                    && job.artifact_sha256.is_none()
                    && job.artifact_byte_length.is_none() =>
            {
                ()
            }
            _ => return Err("A generation has an invalid state transition".into()),
        }
    }
    validate_ancestry(snapshot)?;
    if let Some(id) = &snapshot.trial_success_job {
        let job = snapshot
            .jobs
            .get(id)
            .ok_or("The saved trial refers to an unknown generation")?;
        if job.state != JobState::Succeeded
            || !job.asset.as_ref().is_some_and(|asset| !asset.simulated)
            || job.request.parent_asset_id.is_some()
        {
            return Err("The saved trial must refer to a successful reconstruction".into());
        }
    } else if snapshot.jobs.values().any(|job| {
        job.request.parent_asset_id.is_none()
            && job.asset.as_ref().is_some_and(|asset| !asset.simulated)
    }) {
        return Err(
            "The Sculpt library has inconsistent trial state; it has not been reset".into(),
        );
    }
    Ok(())
}

fn validate_id(id: &str) -> Result<(), String> {
    let parsed = uuid::Uuid::parse_str(id).map_err(|_| "Invalid library identifier")?;
    if parsed.to_string() != id {
        return Err("Library identifiers must be canonical UUIDs".into());
    }
    Ok(())
}

fn validate_request(request: &GenerationRequest) -> Result<(), String> {
    if request.engine_id.is_empty()
        || request.engine_id.len() > 128
        || request.image_name.trim().is_empty()
        || request.image_name.len() > 1024
    {
        return Err("Invalid generation request".into());
    }
    if let Some(id) = &request.source_id {
        validate_id(id)?;
    }
    if request.refinement.is_some() != request.parent_asset_id.is_some() {
        return Err("Refinement settings require a completed parent asset".into());
    }
    if let Some(id) = &request.parent_asset_id {
        validate_id(id)?;
    }
    if let Some(settings) = &request.refinement {
        settings.validate()?;
    }
    Ok(())
}

fn validate_parent(
    snapshot: &Snapshot,
    id: &str,
    request: &GenerationRequest,
) -> Result<(), String> {
    let Some(parent_id) = &request.parent_asset_id else {
        return Ok(());
    };
    let parent = snapshot.jobs.get(parent_id).ok_or("Unknown parent asset")?;
    if id == parent_id
        || parent.state != JobState::Succeeded
        || !parent.asset.as_ref().is_some_and(|asset| !asset.simulated)
        || parent.request.engine_id != request.engine_id
        || parent.request.source_id != request.source_id
    {
        return Err(
            "Refinement requires a completed reconstruction with the same engine and source image"
                .into(),
        );
    }
    Ok(())
}

fn validate_ancestry(snapshot: &Snapshot) -> Result<(), String> {
    // Visit each record once instead of walking every full ancestor chain.
    // Completed-parent checks above do not by themselves reject cycles in a
    // hand-edited snapshot where both jobs are marked successful.
    let mut verified = BTreeSet::new();
    for id in snapshot.jobs.keys() {
        let mut chain = BTreeSet::new();
        let mut cursor = Some(id.as_str());
        while let Some(id) = cursor {
            if verified.contains(id) {
                break;
            }
            if !chain.insert(id) {
                return Err("The refinement history contains a cycle".into());
            }
            cursor = snapshot
                .jobs
                .get(id)
                .ok_or("Unknown parent asset")?
                .request
                .parent_asset_id
                .as_deref();
        }
        verified.extend(chain);
    }
    Ok(())
}

fn validate_source_asset(asset: &SourceAsset) -> Result<(), String> {
    validate_id(&asset.id)?;
    extension(&asset.mime_type)?;
    if !valid_sha(&asset.sha256)
        || asset.width < 16
        || asset.height < 16
        || u64::from(asset.width) * u64::from(asset.height) > 40_000_000
        || asset.name.is_empty()
        || asset.name.len() > 1024
    {
        return Err("Invalid source image record".into());
    }
    Ok(())
}

fn valid_sha(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
}

fn extension(mime: &str) -> Result<&'static str, String> {
    match mime {
        "image/png" => Ok("png"),
        "image/jpeg" => Ok("jpg"),
        "image/webp" => Ok("webp"),
        _ => Err("Unknown source image type".into()),
    }
}

fn ensure_directory(root: &Path, name: &str) -> Result<PathBuf, String> {
    let path = root.join(name);
    if !path.try_exists().map_err(storage_error)? && fs::symlink_metadata(&path).is_err() {
        fs::create_dir(&path).map_err(storage_error)?;
    }
    reject_symlink(&path)?;
    if !path.is_dir() {
        return Err("A Sculpt library directory is not a directory".into());
    }
    Ok(path)
}

fn reject_symlink(path: &Path) -> Result<(), String> {
    let metadata = fs::symlink_metadata(path).map_err(storage_error)?;
    if metadata.file_type().is_symlink() {
        return Err("Symbolic links are not allowed inside the Sculpt library".into());
    }
    Ok(())
}

fn open_private(path: &Path, create: bool) -> Result<File, String> {
    let mut options = OpenOptions::new();
    options.read(true);
    if create {
        options.write(true).create(true);
    }
    #[cfg(unix)]
    options
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    let file = options.open(path).map_err(storage_error)?;
    if !file.metadata().map_err(storage_error)?.is_file() {
        return Err("The library file is not a regular file".into());
    }
    Ok(file)
}

fn verify_source_file(path: &Path, expected_sha: &str) -> Result<(), String> {
    reject_symlink(path)?;
    let mut file = open_private(path, false)?;
    let length = file.metadata().map_err(storage_error)?.len();
    if length == 0 || length > assets::MAX_IMAGE_BYTES as u64 {
        return Err("The stored source image has an invalid size".into());
    }
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    let mut total = 0u64;
    loop {
        let read = file.read(&mut buffer).map_err(storage_error)?;
        if read == 0 {
            break;
        }
        total += read as u64;
        if total > assets::MAX_IMAGE_BYTES as u64 {
            return Err("The stored source image exceeds its size limit".into());
        }
        hash.update(&buffer[..read]);
    }
    if format!("{:x}", hash.finalize()) != expected_sha {
        return Err("The stored source image changed or is damaged; import it again".into());
    }
    file.sync_all().map_err(storage_error)?;
    sync_directory(path.parent().ok_or("Invalid source location")?)
}

fn sync_directory(path: &Path) -> Result<(), String> {
    File::open(path)
        .and_then(|file| file.sync_all())
        .map_err(storage_error)
}

fn now() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .to_string()
}

fn storage_error(error: impl std::fmt::Display) -> String {
    format!("Could not access the Sculpt library: {error}")
}

#[cfg(test)]
mod tests {
    use super::super::runtime::{BackgroundMode, GeometryQuality};
    use super::*;

    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            Self(std::env::temp_dir().join(format!("sculpt-library-{}", uuid::Uuid::new_v4())))
        }
        fn open(&self) -> Library {
            Library::open(self.0.clone()).unwrap()
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn id() -> String {
        uuid::Uuid::new_v4().to_string()
    }
    fn source(library: &mut Library) -> SourceAsset {
        let bytes = b"validated source fixture";
        let asset = SourceAsset {
            id: id(),
            name: "banana.png".into(),
            mime_type: "image/png".into(),
            width: 32,
            height: 32,
            sha256: format!("{:x}", Sha256::digest(bytes)),
        };
        let path = library
            .source_directory()
            .unwrap()
            .join(format!("{}.png", asset.id));
        fs::write(&path, bytes).unwrap();
        library
            .register_source(SourceRecord {
                asset: asset.clone(),
                path,
            })
            .unwrap();
        asset
    }
    fn request(source: Option<&SourceAsset>) -> GenerationRequest {
        GenerationRequest {
            engine_id: if source.is_some() { "triposr" } else { "demo" }.into(),
            image_name: "banana.png".into(),
            source_id: source.map(|source| source.id.clone()),
            geometry: GeometryQuality::Draft,
            background: BackgroundMode::Auto,
            refinement: None,
            parent_asset_id: None,
        }
    }
    fn asset(job: &str, simulated: bool) -> GeneratedAsset {
        GeneratedAsset {
            id: job.into(),
            seed: 1,
            generated_at: now(),
            simulated,
            metrics: None,
        }
    }
    fn write_mesh(library: &Library, job: &str) {
        let directory = library.job_directory(job).unwrap();
        fs::create_dir_all(&directory).unwrap();
        fs::write(directory.join("mesh.glb"), assets::test_triangle_glb()).unwrap();
    }

    fn refine_request(source: &SourceAsset, parent: &str) -> GenerationRequest {
        let mut request = request(Some(source));
        request.parent_asset_id = Some(parent.into());
        request.refinement = Some(super::super::runtime::RefinementSettings {
            resolution: 192,
            density_threshold: 25.0,
            remove_small_components: true,
            smoothing_iterations: 2,
        });
        request
    }

    fn completed_reconstruction(library: &mut Library, source: &SourceAsset) -> String {
        let job = id();
        library.begin_job(&job, request(Some(source))).unwrap();
        write_mesh(library, &job);
        library
            .finish_job(&job, Ok(asset(&job, false)), false)
            .unwrap();
        job
    }

    #[test]
    fn refinements_are_durable_without_spending_another_generation() {
        let fixture = Fixture::new();
        let mut library = fixture.open();
        let source = source(&mut library);
        let original = completed_reconstruction(&mut library, &source);
        let mut parent = original.clone();
        for _ in 0..3 {
            let refined = id();
            library
                .begin_job(&refined, refine_request(&source, &parent))
                .unwrap();
            write_mesh(&library, &refined);
            library
                .finish_job(&refined, Ok(asset(&refined, false)), false)
                .unwrap();
            assert_eq!(library.trial_success_job(), Some(original.as_str()));
            parent = refined;
        }
        drop(library);
        let library = fixture.open();
        assert_eq!(library.trial_success_job(), Some(original.as_str()));
        assert!(library.asset_path(&parent).unwrap().is_file());
        assert!(library.asset_path(&original).unwrap().is_file());
    }

    #[test]
    fn refinement_requires_matching_completed_parent_and_valid_settings() {
        let fixture = Fixture::new();
        let mut library = fixture.open();
        let original_source = source(&mut library);
        let original = completed_reconstruction(&mut library, &original_source);
        let other_source = source(&mut library);
        assert!(library
            .begin_job(&id(), refine_request(&other_source, &original))
            .is_err());
        assert!(library
            .begin_job(&original, refine_request(&original_source, &original))
            .is_err());
        assert!(library
            .begin_job(&id(), refine_request(&original_source, &id()))
            .is_err());
        let mut wrong_engine = refine_request(&original_source, &original);
        wrong_engine.engine_id = "another-engine".into();
        assert!(library.begin_job(&id(), wrong_engine).is_err());
        let mut missing_settings = refine_request(&original_source, &original);
        missing_settings.refinement = None;
        assert!(library.begin_job(&id(), missing_settings).is_err());
        let mut missing_parent = refine_request(&original_source, &original);
        missing_parent.parent_asset_id = None;
        assert!(library.begin_job(&id(), missing_parent).is_err());
        let mut invalid_settings = refine_request(&original_source, &original);
        invalid_settings.refinement.as_mut().unwrap().resolution = 1024;
        assert!(library.begin_job(&id(), invalid_settings).is_err());

        let failed = id();
        library
            .begin_job(&failed, request(Some(&original_source)))
            .unwrap();
        library
            .finish_job(&failed, Err("failed".into()), false)
            .unwrap();
        assert!(library
            .begin_job(&id(), refine_request(&original_source, &failed))
            .is_err());
        fs::write(library.asset_path(&original).unwrap(), b"corrupt").unwrap();
        assert!(library
            .begin_job(&id(), refine_request(&original_source, &original))
            .is_err());
    }

    #[test]
    fn snapshot_rejects_refinement_cycles_and_refinements_claiming_trial() {
        let fixture = Fixture::new();
        let mut library = fixture.open();
        let source = source(&mut library);
        let original = completed_reconstruction(&mut library, &source);
        let refined = id();
        library
            .begin_job(&refined, refine_request(&source, &original))
            .unwrap();
        write_mesh(&library, &refined);
        library
            .finish_job(&refined, Ok(asset(&refined, false)), false)
            .unwrap();
        let mut malformed = library.snapshot.clone();
        malformed.trial_success_job = Some(refined.clone());
        assert!(validate_snapshot(&malformed).unwrap_err().contains("trial"));
        malformed = library.snapshot.clone();
        malformed.jobs.get_mut(&original).unwrap().request = refine_request(&source, &refined);
        assert!(validate_snapshot(&malformed).unwrap_err().contains("cycle"));
    }

    #[test]
    fn previous_generation_request_without_refinement_fields_reopens() {
        let fixture = Fixture::new();
        let mut library = fixture.open();
        let source = source(&mut library);
        let original = completed_reconstruction(&mut library, &source);
        drop(library);
        let path = fixture.0.join("library.json");
        let mut snapshot: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        let request = snapshot["jobs"][&original]["request"]
            .as_object_mut()
            .unwrap();
        request.remove("parentAssetId");
        request.remove("refinement");
        fs::write(&path, serde_json::to_vec(&snapshot).unwrap()).unwrap();
        let library = fixture.open();
        assert!(library.asset_path(&original).unwrap().is_file());
        assert_eq!(library.trial_success_job(), Some(original.as_str()));
    }

    #[test]
    fn restart_preserves_success_sources_license_and_trial() {
        let fixture = Fixture::new();
        let mut library = fixture.open();
        let source = source(&mut library);
        let job = id();
        library.begin_job(&job, request(Some(&source))).unwrap();
        write_mesh(&library, &job);
        library
            .finish_job(&job, Ok(asset(&job, false)), false)
            .unwrap();
        library
            .set_signed_license(Some("opaque.signed.license".into()))
            .unwrap();
        drop(library);
        let library = fixture.open();
        assert_eq!(
            library.source(&source.id).unwrap().asset.sha256,
            source.sha256
        );
        assert_eq!(library.job(&job).unwrap().state, JobState::Succeeded);
        assert!(library.asset_path(&job).unwrap().is_file());
        assert_eq!(library.trial_success_job(), Some(job.as_str()));
        assert_eq!(library.signed_license(), Some("opaque.signed.license"));
    }

    #[test]
    fn interrupted_failed_cancelled_and_demo_do_not_consume_trial() {
        let fixture = Fixture::new();
        let mut library = fixture.open();
        let source = source(&mut library);
        let interrupted = id();
        library
            .begin_job(&interrupted, request(Some(&source)))
            .unwrap();
        drop(library);
        let mut library = fixture.open();
        assert_eq!(
            library.job(&interrupted).unwrap().state,
            JobState::Interrupted
        );
        for cancelled in [false, true] {
            let job = id();
            library.begin_job(&job, request(Some(&source))).unwrap();
            let record = library
                .finish_job(&job, Err("worker failed".into()), cancelled)
                .unwrap();
            assert_eq!(
                record.state,
                if cancelled {
                    JobState::Cancelled
                } else {
                    JobState::Failed
                }
            );
        }
        let demo = id();
        library.begin_job(&demo, request(None)).unwrap();
        library
            .finish_job(&demo, Ok(asset(&demo, true)), false)
            .unwrap();
        assert!(library.trial_success_job().is_none());
        drop(library);
        assert!(fixture.open().trial_success_job().is_none());
    }

    #[test]
    fn completion_is_idempotent_and_rejects_job_reuse_and_mutation() {
        let fixture = Fixture::new();
        let mut library = fixture.open();
        let job = id();
        library.begin_job(&job, request(None)).unwrap();
        let result = asset(&job, true);
        let first = library.finish_job(&job, Ok(result.clone()), false).unwrap();
        let second = library.finish_job(&job, Ok(result), false).unwrap();
        assert_eq!(first.updated_at, second.updated_at);
        assert!(library.begin_job(&job, request(None)).is_err());
        assert!(library
            .finish_job(&job, Err("late error".into()), false)
            .is_err());
        assert!(library
            .finish_job(&job, Ok(asset(&job, true)), true)
            .is_err());
    }

    #[test]
    fn missing_or_invalid_mesh_cannot_consume_trial() {
        let fixture = Fixture::new();
        let mut library = fixture.open();
        let source = source(&mut library);
        let job = id();
        library.begin_job(&job, request(Some(&source))).unwrap();
        assert!(library
            .finish_job(&job, Ok(asset(&job, false)), false)
            .is_err());
        fs::create_dir_all(library.job_directory(&job).unwrap()).unwrap();
        fs::write(
            library.job_directory(&job).unwrap().join("mesh.glb"),
            b"bad",
        )
        .unwrap();
        assert!(library
            .finish_job(&job, Ok(asset(&job, false)), false)
            .is_err());
        assert_eq!(library.job(&job).unwrap().state, JobState::Running);
        assert!(library.trial_success_job().is_none());
    }

    #[test]
    fn corrupt_and_future_snapshot_fail_closed_without_resetting_bytes() {
        for bytes in [b"{broken".as_slice(), br#"{"version":999,"sources":{},"jobs":{},"signedLicense":null,"trialSuccessJob":null}"#.as_slice()] {
            let fixture = Fixture::new();
            drop(fixture.open());
            let path = fixture.0.join("library.json");
            fs::write(&path, bytes).unwrap();
            assert!(Library::open(fixture.0.clone()).is_err());
            assert_eq!(fs::read(&path).unwrap(), bytes);
        }
    }

    #[test]
    fn changed_source_and_path_traversal_are_rejected() {
        let fixture = Fixture::new();
        let mut library = fixture.open();
        let source = source(&mut library);
        let path = library.source(&source.id).unwrap().path;
        fs::write(path, b"changed").unwrap();
        assert!(library.source(&source.id).is_err());
        assert!(library.source("../outside").is_err());
        assert!(library.job_directory("../outside").is_err());
        assert!(library.begin_job(&id(), request(Some(&source))).is_err());
    }

    #[test]
    fn saved_asset_integrity_is_checked_after_restart() {
        let fixture = Fixture::new();
        let mut library = fixture.open();
        let source = source(&mut library);
        let job = id();
        library.begin_job(&job, request(Some(&source))).unwrap();
        write_mesh(&library, &job);
        library
            .finish_job(&job, Ok(asset(&job, false)), false)
            .unwrap();
        let path = library.asset_path(&job).unwrap();
        let mut bytes = fs::read(&path).unwrap();
        let last = bytes.len() - 1;
        bytes[last] ^= 1;
        fs::write(&path, bytes).unwrap();
        drop(library);
        let library = fixture.open();
        assert!(library.asset_path(&job).is_err());
        assert_eq!(library.trial_success_job(), Some(job.as_str()));
    }

    #[test]
    fn completed_asset_remains_available_when_source_file_is_missing() {
        let fixture = Fixture::new();
        let mut library = fixture.open();
        let source = source(&mut library);
        let job = id();
        library.begin_job(&job, request(Some(&source))).unwrap();
        write_mesh(&library, &job);
        library
            .finish_job(&job, Ok(asset(&job, false)), false)
            .unwrap();
        let source_path = library.source(&source.id).unwrap().path;
        fs::remove_file(source_path).unwrap();
        drop(library);
        let library = fixture.open();
        assert!(library.source(&source.id).is_err());
        assert!(library.asset_path(&job).unwrap().is_file());
        assert_eq!(library.trial_success_job(), Some(job.as_str()));
    }

    #[test]
    fn inconsistent_trial_and_oversized_snapshot_are_not_reset() {
        let fixture = Fixture::new();
        let mut library = fixture.open();
        let source = source(&mut library);
        let job = id();
        library.begin_job(&job, request(Some(&source))).unwrap();
        write_mesh(&library, &job);
        library
            .finish_job(&job, Ok(asset(&job, false)), false)
            .unwrap();
        drop(library);
        let path = fixture.0.join("library.json");
        let mut saved: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        saved["trialSuccessJob"] = serde_json::Value::Null;
        let bytes = serde_json::to_vec(&saved).unwrap();
        fs::write(&path, &bytes).unwrap();
        assert!(Library::open(fixture.0.clone()).is_err());
        assert_eq!(fs::read(&path).unwrap(), bytes);
        OpenOptions::new()
            .write(true)
            .open(&path)
            .unwrap()
            .set_len(MAX_SNAPSHOT_BYTES + 1)
            .unwrap();
        assert!(Library::open(fixture.0.clone()).is_err());
        assert_eq!(fs::metadata(&path).unwrap().len(), MAX_SNAPSHOT_BYTES + 1);
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlinked_source_mesh_snapshot_and_output_directory() {
        use std::os::unix::fs::symlink;
        let fixture = Fixture::new();
        let mut library = fixture.open();
        let source = source(&mut library);
        let job = id();
        library.begin_job(&job, request(Some(&source))).unwrap();
        let source_path = library.source(&source.id).unwrap().path;
        let outside = fixture.0.join("outside.png");
        fs::rename(&source_path, &outside).unwrap();
        symlink(&outside, &source_path).unwrap();
        assert!(library.source(&source.id).is_err());
        fs::remove_file(&source_path).unwrap();
        fs::rename(&outside, &source_path).unwrap();
        let directory = library.job_directory(&job).unwrap();
        symlink(library.source_directory().unwrap(), &directory).unwrap();
        assert!(library.job_directory(&job).is_err());
        fs::remove_file(&directory).unwrap();
        write_mesh(&library, &job);
        let mesh = directory.join("mesh.glb");
        fs::remove_file(&mesh).unwrap();
        symlink(&source_path, &mesh).unwrap();
        assert!(library
            .finish_job(&job, Ok(asset(&job, false)), false)
            .is_err());
        drop(library);
        let snapshot = fixture.0.join("library.json");
        fs::remove_file(&snapshot).unwrap();
        symlink(fixture.0.join("missing.json"), &snapshot).unwrap();
        assert!(Library::open(fixture.0.clone()).is_err());
    }

    #[test]
    fn failed_persistence_does_not_publish_memory_state() {
        let fixture = Fixture::new();
        let mut library = fixture.open();
        let path = fixture.0.join("library.json");
        fs::remove_file(&path).unwrap();
        fs::create_dir(&path).unwrap();
        let job = id();
        assert!(library.begin_job(&job, request(None)).is_err());
        assert!(library.job(&job).is_none());
        assert!(library
            .set_signed_license(Some("not.saved".into()))
            .is_err());
        assert!(library.signed_license().is_none());
        assert!(library.trial_success_job().is_none());
    }

    #[test]
    fn failed_success_commit_keeps_trial_and_running_record_unchanged() {
        let fixture = Fixture::new();
        let mut library = fixture.open();
        let source = source(&mut library);
        let job = id();
        library.begin_job(&job, request(Some(&source))).unwrap();
        write_mesh(&library, &job);
        let path = fixture.0.join("library.json");
        let previous = fs::read(&path).unwrap();
        fs::remove_file(&path).unwrap();
        fs::create_dir(&path).unwrap();
        assert!(library
            .finish_job(&job, Ok(asset(&job, false)), false)
            .is_err());
        assert_eq!(library.job(&job).unwrap().state, JobState::Running);
        assert!(library.trial_success_job().is_none());
        fs::remove_dir(&path).unwrap();
        fs::write(&path, previous).unwrap();
        drop(library);
        let library = fixture.open();
        assert_eq!(library.job(&job).unwrap().state, JobState::Interrupted);
        assert!(library.trial_success_job().is_none());
    }

    #[cfg(unix)]
    #[test]
    fn root_lock_and_owned_directories_cannot_be_symlinks() {
        use std::os::unix::fs::symlink;
        let fixture = Fixture::new();
        fs::create_dir_all(&fixture.0).unwrap();
        let actual = fixture.0.join("actual");
        fs::create_dir(&actual).unwrap();
        let alias = fixture.0.join("alias");
        symlink(&actual, &alias).unwrap();
        assert!(Library::open(alias).is_err());
        let lock = fixture.0.join(".library.lock");
        let outside = fixture.0.join("unrelated");
        fs::write(&outside, b"unchanged").unwrap();
        symlink(&outside, &lock).unwrap();
        assert!(Library::open(fixture.0.clone()).is_err());
        assert_eq!(fs::read(&outside).unwrap(), b"unchanged");
        fs::remove_file(lock).unwrap();
        symlink(&actual, fixture.0.join("sources")).unwrap();
        assert!(Library::open(fixture.0.clone()).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn lifetime_lock_rejects_second_owner_until_first_closes() {
        const CHILD_ROOT: &str = "SCULPT_LIBRARY_LOCK_TEST_ROOT";
        if let Some(root) = std::env::var_os(CHILD_ROOT) {
            assert!(Library::open(PathBuf::from(root)).is_err());
            return;
        }
        let fixture = Fixture::new();
        let library = fixture.open();
        assert!(Library::open(fixture.0.clone()).is_err());
        let child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "harness::library::tests::lifetime_lock_rejects_second_owner_until_first_closes",
            ])
            .env(CHILD_ROOT, &fixture.0)
            .output()
            .unwrap();
        assert!(
            child.status.success(),
            "{}",
            String::from_utf8_lossy(&child.stdout)
        );
        drop(library);
        assert!(Library::open(fixture.0.clone()).is_ok());
    }
}
