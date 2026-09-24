//! Native state-boundary tests: no Python environment or GPU required.
use super::*;
use assets::{SourceAsset, SourceRecord};
use library::{JobRecord, JobState, Library};
use runtime::{BackgroundMode, GeometryQuality};
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};

struct Fixture {
    root: PathBuf,
    jobs: Arc<Mutex<HashMap<String, Arc<AtomicBool>>>>,
    library: Arc<Mutex<Result<Library, String>>>,
    request: GenerationRequest,
}

impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("sculpt-lifecycle-{}", uuid::Uuid::new_v4()));
        let mut library = Library::open(root.clone()).unwrap();
        // The importer is tested independently; this fixture begins at the
        // native library boundary with a registered, hash-identified source.
        let bytes = b"registered source fixture";
        let source = SourceAsset {
            id: uuid::Uuid::new_v4().to_string(),
            name: "banana.png".into(),
            mime_type: "image/png".into(),
            width: 32,
            height: 32,
            sha256: format!("{:x}", Sha256::digest(bytes)),
        };
        let source_path = library
            .source_directory()
            .unwrap()
            .join(format!("{}.png", source.id));
        fs::write(&source_path, bytes).unwrap();
        library
            .register_source(SourceRecord {
                asset: source.clone(),
                path: source_path,
            })
            .unwrap();
        Self {
            root,
            jobs: Arc::new(Mutex::new(HashMap::new())),
            library: Arc::new(Mutex::new(Ok(library))),
            request: GenerationRequest {
                engine_id: "triposr".into(),
                image_name: source.name,
                source_id: Some(source.id),
                mask_sha256: None,
                geometry: GeometryQuality::Draft,
                background: BackgroundMode::Auto,
                parent_asset_id: None,
                refinement: None,
            },
        }
    }

    fn reserve(&self) -> JobReservation {
        let id = uuid::Uuid::new_v4().to_string();
        with_library(&self.library, |library| {
            library.begin_job(&id, self.request.clone())
        })
        .unwrap();
        let cancelled = Arc::new(AtomicBool::new(false));
        self.jobs
            .lock()
            .unwrap()
            .insert(id.clone(), cancelled.clone());
        JobReservation {
            id,
            jobs: self.jobs.clone(),
            library: self.library.clone(),
            cancelled,
            finalized: false,
        }
    }

    fn output(&self, id: &str) -> PathBuf {
        let directory = with_library(&self.library, |library| library.job_directory(id)).unwrap();
        fs::create_dir_all(&directory).unwrap();
        directory
    }

    fn write_mesh(&self, id: &str) {
        fs::write(
            self.output(id).join("mesh.glb"),
            assets::test_triangle_glb(),
        )
        .unwrap();
    }

    fn job(&self, id: &str) -> JobRecord {
        with_library(&self.library, |library| Ok(library.job(id).unwrap())).unwrap()
    }

    fn trial(&self) -> Option<String> {
        with_library(&self.library, |library| {
            Ok(library.trial_success_job().map(str::to_owned))
        })
        .unwrap()
    }

    fn complete_with_cache(&self, bytes: &[u8], include_hash: bool) -> String {
        let mut reservation = self.reserve();
        let id = reservation.id.clone();
        self.write_mesh(&id);
        fs::write(self.output(&id).join("scene-cache.npz"), bytes).unwrap();
        let mut result = generated(&id);
        if include_hash {
            result.metrics = Some(
                serde_json::json!({"sceneCacheSha256": format!("{:x}", Sha256::digest(bytes))}),
            );
        }
        reservation.finish(Ok(result)).unwrap();
        id
    }

    fn cache(&self, id: &str) -> Result<PathBuf, String> {
        with_library(&self.library, |library| verified_scene_cache(library, id))
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        // Reservations are dropped before their fixture in every test.
        *self.library.lock().unwrap() = Err("Fixture closed".into());
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn generated(id: &str) -> GeneratedAsset {
    GeneratedAsset {
        id: id.into(),
        seed: 0,
        generated_at: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis()
            .to_string(),
        simulated: false,
        metrics: None,
    }
}

#[test]
fn invalid_worker_success_releases_slot_without_spending_trial() {
    let fixture = Fixture::new();
    let mut reservation = fixture.reserve();
    let id = reservation.id.clone();
    fs::write(fixture.output(&id).join("mesh.glb"), b"not a GLB").unwrap();
    assert!(reservation.finish(Ok(generated(&id))).is_err());
    assert!(!reservation.finalized);
    assert!(fixture.trial().is_none());
    drop(reservation);
    assert!(fixture.jobs.lock().unwrap().is_empty());
    assert_eq!(fixture.job(&id).state, JobState::Failed);
    assert!(fixture.trial().is_none());
    // The failed commit cannot leave the library or active-operation slot stuck.
    let retry = fixture.reserve();
    assert_eq!(fixture.job(&retry.id).state, JobState::Running);
}

#[test]
fn cancellation_before_commit_discards_worker_success_without_spending_trial() {
    let fixture = Fixture::new();
    let mut reservation = fixture.reserve();
    let id = reservation.id.clone();
    fixture.write_mesh(&id);
    reservation.cancelled.store(true, Ordering::Relaxed);
    assert_eq!(
        reservation.finish(Ok(generated(&id))).unwrap_err(),
        "Generation cancelled"
    );
    assert!(reservation.finalized);
    drop(reservation);
    assert!(fixture.jobs.lock().unwrap().is_empty());
    assert_eq!(fixture.job(&id).state, JobState::Cancelled);
    assert!(fixture.trial().is_none());
    assert!(with_library(&fixture.library, |library| library.asset_path(&id)).is_err());
}

#[test]
fn cancellation_after_commit_does_not_erase_asset_or_restore_trial() {
    let fixture = Fixture::new();
    let mut reservation = fixture.reserve();
    let id = reservation.id.clone();
    fixture.write_mesh(&id);
    reservation.finish(Ok(generated(&id))).unwrap();
    reservation.cancelled.store(true, Ordering::Relaxed);
    drop(reservation);
    assert!(fixture.jobs.lock().unwrap().is_empty());
    assert_eq!(fixture.job(&id).state, JobState::Succeeded);
    assert_eq!(fixture.trial().as_deref(), Some(id.as_str()));
    assert!(with_library(&fixture.library, |library| library.asset_path(&id)).is_ok());
    assert!(with_library(&fixture.library, |library| library.finish_job(
        &id,
        Err("late cancel".into()),
        true
    ))
    .is_err());
    assert_eq!(fixture.job(&id).state, JobState::Succeeded);
}

#[test]
fn worker_failure_and_abandoned_reservation_both_release_active_slot() {
    let fixture = Fixture::new();
    let mut reservation = fixture.reserve();
    let id = reservation.id.clone();
    assert_eq!(
        reservation.finish(Err("Worker exited".into())).unwrap_err(),
        "Worker exited"
    );
    drop(reservation);
    assert_eq!(fixture.job(&id).error.as_deref(), Some("Worker exited"));
    assert_eq!(fixture.job(&id).state, JobState::Failed);
    assert!(fixture.jobs.lock().unwrap().is_empty());
    let abandoned = fixture.reserve();
    let abandoned_id = abandoned.id.clone();
    drop(abandoned);
    assert_eq!(fixture.job(&abandoned_id).state, JobState::Failed);
    assert!(fixture.jobs.lock().unwrap().is_empty());
    assert!(fixture.trial().is_none());
}

#[test]
fn refinement_rejects_missing_or_changed_cache_and_accepts_verified_bytes() {
    let fixture = Fixture::new();
    let id = fixture.complete_with_cache(b"original numeric cache fixture", true);
    let path = fixture.cache(&id).unwrap();
    fs::write(&path, b"modified numeric cache fixture").unwrap();
    assert!(fixture
        .cache(&id)
        .unwrap_err()
        .contains("changed or is damaged"));
    fs::remove_file(&path).unwrap();
    assert!(fixture.cache(&id).unwrap_err().contains("missing"));
    // A lost refinement cache must not invalidate the completed/exportable mesh.
    assert!(with_library(&fixture.library, |library| library.asset_path(&id)).is_ok());
}

#[test]
fn older_assets_without_cache_identity_get_actionable_error() {
    let fixture = Fixture::new();
    let id = fixture.complete_with_cache(b"unidentified cache", false);
    assert!(fixture
        .cache(&id)
        .unwrap_err()
        .contains("predates refinement"));
}

#[test]
fn refinement_rejects_oversized_cache_before_reading_it() {
    let fixture = Fixture::new();
    let id = fixture.complete_with_cache(b"valid initial cache", true);
    let path = fixture.cache(&id).unwrap();
    fs::OpenOptions::new()
        .write(true)
        .open(path)
        .unwrap()
        .set_len(4 * 1024 * 1024 + 1)
        .unwrap();
    assert!(fixture.cache(&id).unwrap_err().contains("invalid"));
}

#[cfg(unix)]
#[test]
fn refinement_rejects_cache_symlinks_even_when_target_hash_matches() {
    let fixture = Fixture::new();
    let id = fixture.complete_with_cache(b"valid initial cache", true);
    let path = fixture.cache(&id).unwrap();
    let target = fixture.root.join("other-cache.npz");
    fs::rename(&path, &target).unwrap();
    std::os::unix::fs::symlink(&target, &path).unwrap();
    assert!(fixture.cache(&id).unwrap_err().contains("invalid"));
}
