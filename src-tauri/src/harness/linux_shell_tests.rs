//! Focused Ubuntu 22.04 desktop shell, .deb packaging config, logging, and library smoke tests.
use super::*;
use assets::decode_source;
use library::Library;
use logging::HostLogger;
use runtime::{BackgroundMode, GeometryQuality, MockRuntime};
use std::path::Path;

fn ubuntu_2204_os_release() -> String {
    r#"PRETTY_NAME="Ubuntu 22.04.4 LTS"
NAME="Ubuntu"
VERSION_ID="22.04"
VERSION="22.04.4 LTS (Jammy Jellyfish)"
VERSION_CODENAME=jammy
ID=ubuntu
ID_LIKE=debian"#
        .into()
}

fn ubuntu_2204_cuda_profile() -> hardware::HardwareProfile {
    hardware::profile_from_linux_probes(&hardware::LinuxProbes {
        os_release: Some(ubuntu_2204_os_release()),
        cpuinfo: Some("model name\t: AMD Ryzen 9 7950X 16-Core Processor\n".into()),
        meminfo: Some("MemTotal:       32645120 kB\n".into()),
        df_output: None,
        storage_total_bytes: Some(500 * 1024 * 1024 * 1024),
        architecture: Some("x86_64".into()),
        gpu_name: Some("NVIDIA GeForce RTX 4080".into()),
        has_nvidia: true,
        has_vulkan: true,
        is_unified_memory: false,
    })
}

fn ubuntu_2204_cpu_profile() -> hardware::HardwareProfile {
    hardware::profile_from_linux_probes(&hardware::LinuxProbes {
        os_release: Some(ubuntu_2204_os_release()),
        cpuinfo: Some("model name\t: Intel(R) Xeon(R) Platinum 8370C CPU @ 2.80GHz\n".into()),
        meminfo: Some("MemTotal:       16384000 kB\n".into()),
        df_output: None,
        storage_total_bytes: Some(128 * 1024 * 1024 * 1024),
        architecture: Some("x86_64".into()),
        gpu_name: None,
        has_nvidia: false,
        has_vulkan: false,
        is_unified_memory: false,
    })
}

#[test]
fn ubuntu_2204_cuda_shell_marks_triposr_experimental_and_recommends_demo() {
    let profile = ubuntu_2204_cuda_profile();
    assert_eq!(profile.os, "Ubuntu");
    assert_eq!(profile.os_version, "Ubuntu 22.04.4 LTS");
    assert_eq!(profile.architecture, "x86_64");
    assert!(!profile.is_apple_silicon);
    assert_eq!(profile.detection_source, "native");
    assert_eq!(profile.compute_backends, vec!["CUDA", "Vulkan", "CPU"]);

    let catalog = catalog::engines(&profile);
    let demo = catalog
        .iter()
        .find(|e| e.id == "demo")
        .expect("demo engine");
    assert_eq!(demo.compatibility, "recommended");
    assert_eq!(
        demo.compatibility_status,
        engines::CompatibilityStatus::Supported
    );

    let triposr = catalog
        .iter()
        .find(|e| e.id == "triposr")
        .expect("triposr engine");
    assert_eq!(triposr.compatibility, "available");
    assert_eq!(
        triposr.compatibility_status,
        engines::CompatibilityStatus::Experimental
    );
    assert!(engines::resolve("triposr", &profile).is_err());

    let trellis = catalog
        .iter()
        .find(|e| e.id == "trellis-2")
        .expect("trellis-2 engine");
    assert_eq!(trellis.compatibility, "unsupported");
}

#[test]
fn ubuntu_2204_cpu_only_shell_marks_triposr_unsupported_and_recommends_demo() {
    let profile = ubuntu_2204_cpu_profile();
    assert_eq!(profile.os, "Ubuntu");
    assert_eq!(profile.compute_backends, vec!["CPU"]);

    let catalog = catalog::engines(&profile);
    let demo = catalog
        .iter()
        .find(|e| e.id == "demo")
        .expect("demo engine");
    assert_eq!(demo.compatibility, "recommended");
    assert_eq!(
        demo.compatibility_status,
        engines::CompatibilityStatus::Supported
    );

    let triposr = catalog
        .iter()
        .find(|e| e.id == "triposr")
        .expect("triposr engine");
    assert_eq!(triposr.compatibility, "unsupported");
    assert_eq!(
        triposr.compatibility_status,
        engines::CompatibilityStatus::Unsupported
    );
    assert!(engines::resolve("triposr", &profile).is_err());
}

#[test]
fn linux_tauri_deb_bundle_and_capabilities_are_valid() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let base_conf: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(manifest_dir.join("tauri.conf.json")).unwrap(),
    )
    .expect("valid tauri.conf.json");
    let linux_conf: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(manifest_dir.join("tauri.linux.conf.json")).unwrap(),
    )
    .expect("valid tauri.linux.conf.json");

    assert_eq!(base_conf["build"]["devUrl"], "http://127.0.0.1:1420");
    assert_eq!(linux_conf["bundle"]["targets"], serde_json::json!(["deb"]));
    assert_eq!(linux_conf["bundle"]["linux"]["deb"]["section"], "graphics");
    assert_eq!(linux_conf["bundle"]["linux"]["deb"]["priority"], "optional");

    let icons = linux_conf["bundle"]["icon"]
        .as_array()
        .expect("linux bundle icon array");
    assert!(!icons.is_empty());
    for icon in icons {
        let rel = icon.as_str().expect("icon path string");
        assert!(
            manifest_dir.join(rel).is_file(),
            "Missing Linux icon file: {rel}"
        );
    }

    let cap: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(manifest_dir.join("capabilities/default.json")).unwrap(),
    )
    .expect("valid capabilities/default.json");
    let perms = cap["permissions"].as_array().unwrap();
    assert!(perms.iter().any(|p| p == "core:default"));
    assert!(perms
        .iter()
        .any(|p| p == "core:window:allow-start-dragging"));
    assert!(cap.get("platforms").is_none());
}

#[test]
fn linux_shell_import_demo_library_and_logging_smoke() {
    let uv = setup::uv_release_for("linux", "x86_64").expect("Linux x64 uv release");
    assert_eq!(uv.archive_filename, "uv-x86_64-unknown-linux-gnu.tar.gz");
    assert_eq!(uv.binary_name, "uv");
    assert!(!uv.is_zip);
    assert_eq!(
        paths::python_executable_for_platform(Path::new("/tmp/sculpt-runtime"), false),
        Path::new("/tmp/sculpt-runtime/venv/bin/python")
    );
    assert_eq!(
        hardware::execution_mode_for("x86_64", "x86_64"),
        "native-x64"
    );
    assert!(!hardware::is_emulated_execution("x86_64", "x86_64"));

    let root = std::env::temp_dir().join(format!("sculpt-linux-shell-{}", uuid::Uuid::new_v4()));
    let log_root = root.join("logs");
    let lib_root = root.join("library");

    let logger = HostLogger::new(Ok(log_root.clone()));
    logger.info("shell", "Ubuntu 22.04 x64 smoke launch");

    let mut library = Library::open(lib_root.clone()).expect("open Linux library");
    let png_16x16 = {
        use image::{ImageBuffer, ImageFormat, Rgba};
        let img: ImageBuffer<Rgba<u8>, Vec<u8>> =
            ImageBuffer::from_pixel(16, 16, Rgba([150, 180, 160, 255]));
        let mut cursor = std::io::Cursor::new(Vec::new());
        img.write_to(&mut cursor, ImageFormat::Png).unwrap();
        use base64::Engine;
        format!(
            "data:image/png;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(cursor.into_inner())
        )
    };
    let source_dir = library.source_directory().unwrap();
    let record = decode_source("study.png".into(), &png_16x16, &source_dir).expect("decode source");
    let source_id = record.asset.id.clone();
    library.register_source(record).expect("register source");

    let job_id = uuid::Uuid::new_v4().to_string();
    let request = GenerationRequest {
        engine_id: "demo".into(),
        image_name: "study.png".into(),
        source_id: Some(source_id.clone()),
        mask_sha256: None,
        geometry: GeometryQuality::Draft,
        background: BackgroundMode::Keep,
        parent_asset_id: None,
        refinement: None,
    };
    library.begin_job(&job_id, request.clone()).unwrap();
    let context = JobContext {
        id: job_id.clone(),
        cancelled: Arc::new(AtomicBool::new(false)),
        progress: Arc::new(|_| {}),
    };
    let asset = tauri::async_runtime::block_on(MockRuntime.generate(request, context)).unwrap();
    assert!(asset.simulated);
    library.finish_job(&job_id, Ok(asset), false).unwrap();
    drop(library);

    let reopened = Library::open(lib_root).expect("reopen Linux library");
    let jobs = reopened.list_jobs(10);
    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0].id, job_id);
    assert!(reopened.source(&source_id).is_ok());

    let log_text = std::fs::read_to_string(log_root.join("sculpt.log")).unwrap();
    assert!(log_text.contains("Ubuntu 22.04 x64 smoke launch"));
    assert!(!log_text.contains("study.png"));

    drop(reopened);
    let _ = std::fs::remove_dir_all(root);
}
