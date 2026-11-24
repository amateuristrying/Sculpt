//! Focused Windows x64 desktop shell, packaging config, logging, and library smoke tests.
use super::*;
use assets::decode_source;
use library::Library;
use logging::HostLogger;
use runtime::{BackgroundMode, GeometryQuality, MockRuntime};
use std::path::Path;

fn windows_x64_profile() -> hardware::HardwareProfile {
    hardware::profile_from_windows_probes(&hardware::windows::WindowsProbes {
        os_name: Some("Windows 11 Pro".into()),
        display_version: Some("24H2".into()),
        build_number: Some("26100".into()),
        ubr: Some(1),
        architecture: Some("AMD64".into()),
        cpu_name: Some("AMD Ryzen 9 7950X".into()),
        total_ram_bytes: Some(32 * 1024 * 1024 * 1024),
        storage_total_bytes: Some(1024 * 1024 * 1024 * 1024),
        storage_free_bytes: Some(400 * 1024 * 1024 * 1024),
        gpus: vec![hardware::windows::WindowsGpuAdapter {
            name: "NVIDIA GeForce RTX 4080".into(),
            provider: Some("NVIDIA".into()),
            dedicated_vram_bytes: Some(16 * 1024 * 1024 * 1024),
            shared_ram_bytes: None,
            driver_version: Some("566.03".into()),
        }],
        npu_name: None,
        has_directml: true,
        has_vulkan: true,
        has_cuda: true,
    })
}

#[test]
fn windows_x64_shell_recommends_demo_without_inference_claims() {
    let profile = windows_x64_profile();
    assert_eq!(profile.os, "Windows");
    assert_eq!(profile.architecture, "x86_64");
    assert!(!profile.is_apple_silicon);
    assert_eq!(profile.detection_source, "native");

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
    assert_ne!(triposr.compatibility, "recommended");
}

fn windows_arm64_snapdragon_profile() -> hardware::HardwareProfile {
    hardware::profile_from_windows_probes(&hardware::windows::WindowsProbes {
        os_name: Some("Windows 11 Home".into()),
        display_version: Some("24H2".into()),
        build_number: Some("26100".into()),
        ubr: Some(1000),
        architecture: Some("ARM64".into()),
        cpu_name: Some("Snapdragon(R) X Plus - X1P42100 - Qualcomm(R) Oryon(TM) CPU".into()),
        total_ram_bytes: Some(16 * 1024 * 1024 * 1024),
        storage_total_bytes: Some(1024 * 1024 * 1024 * 1024),
        storage_free_bytes: Some(500 * 1024 * 1024 * 1024),
        gpus: vec![hardware::windows::WindowsGpuAdapter {
            name: "Qualcomm(R) Adreno(TM) X1-45 GPU".into(),
            provider: Some("Qualcomm Incorporated".into()),
            dedicated_vram_bytes: None,
            shared_ram_bytes: None,
            driver_version: Some("31.0.82.0".into()),
        }],
        npu_name: Some("Qualcomm(R) Hexagon(TM) NPU".into()),
        has_directml: true,
        has_vulkan: true,
        has_cuda: false,
    })
}

#[test]
fn windows_arm64_snapdragon_shell_recommends_demo_without_inference_claims() {
    let profile = windows_arm64_snapdragon_profile();
    assert_eq!(profile.os, "Windows");
    assert_eq!(profile.architecture, "arm64");
    assert!(profile.unified_memory);
    assert!(!profile.is_apple_silicon);
    assert_eq!(profile.detection_source, "native");
    assert_eq!(profile.compute_backends, vec!["DirectML", "Vulkan", "CPU"]);
    assert_eq!(profile.npu.as_deref(), Some("Qualcomm(R) Hexagon(TM) NPU"));

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

    let uv = setup::uv_release_for("windows", "aarch64").expect("Windows ARM64 uv release");
    assert_eq!(uv.archive_filename, "uv-aarch64-pc-windows-msvc.zip");
    assert_eq!(uv.binary_name, "uv.exe");
}

#[test]
fn windows_execution_mode_separates_native_arm64_from_x64_emulation() {
    assert_eq!(
        hardware::execution_mode_for("aarch64", "arm64"),
        "native-arm64"
    );
    assert!(!hardware::is_emulated_execution("aarch64", "arm64"));

    assert_eq!(
        hardware::execution_mode_for("x86_64", "arm64"),
        "x64-emulation-on-arm64"
    );
    assert!(hardware::is_emulated_execution("x86_64", "arm64"));

    assert_eq!(
        hardware::execution_mode_for("x86_64", "x86_64"),
        "native-x64"
    );
    assert!(!hardware::is_emulated_execution("x86_64", "x86_64"));

    #[cfg(all(target_os = "windows", target_arch = "aarch64"))]
    {
        let profile = hardware::detect();
        assert_eq!(profile.architecture, "arm64");
        assert_eq!(
            hardware::execution_mode_for(std::env::consts::ARCH, &profile.architecture),
            "native-arm64"
        );
        assert!(!hardware::is_emulated_execution(
            std::env::consts::ARCH,
            &profile.architecture
        ));
    }
}

#[test]
fn windows_tauri_bundle_and_capabilities_are_valid() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let base_conf: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(manifest_dir.join("tauri.conf.json")).unwrap(),
    )
    .expect("valid tauri.conf.json");
    let win_conf: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(manifest_dir.join("tauri.windows.conf.json")).unwrap(),
    )
    .expect("valid tauri.windows.conf.json");

    assert_eq!(base_conf["build"]["devUrl"], "http://127.0.0.1:1420");
    assert!(base_conf["bundle"]["icon"]
        .as_array()
        .unwrap()
        .iter()
        .any(|icon| icon == "icons/icon.ico"));

    assert_eq!(win_conf["bundle"]["targets"], serde_json::json!(["nsis"]));
    assert_eq!(
        win_conf["bundle"]["windows"]["nsis"]["installMode"],
        "currentUser"
    );
    assert_eq!(win_conf["bundle"]["windows"]["nsis"]["compression"], "lzma");
    assert_eq!(
        win_conf["bundle"]["windows"]["webviewInstallMode"]["type"],
        "downloadBootstrapper"
    );
    assert!(manifest_dir.join("icons/icon.ico").is_file());

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
fn windows_shell_import_demo_library_and_logging_smoke() {
    let root = std::env::temp_dir().join(format!("sculpt-win-shell-{}", uuid::Uuid::new_v4()));
    let log_root = root.join("logs");
    let lib_root = root.join("library");

    let logger = HostLogger::new(Ok(log_root.clone()));
    logger.info("shell", "Windows x64 smoke launch");

    let mut library = Library::open(lib_root.clone()).expect("open Windows library");
    let png_16x16 = {
        use image::{ImageBuffer, ImageFormat, Rgba};
        let img: ImageBuffer<Rgba<u8>, Vec<u8>> =
            ImageBuffer::from_pixel(16, 16, Rgba([180, 200, 170, 255]));
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

    let reopened = Library::open(lib_root).expect("reopen Windows library");
    let jobs = reopened.list_jobs(10);
    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0].id, job_id);
    assert!(reopened.source(&source_id).is_ok());

    let log_text = std::fs::read_to_string(log_root.join("sculpt.log")).unwrap();
    assert!(log_text.contains("Windows x64 smoke launch"));
    assert!(!log_text.contains("study.png"));

    drop(reopened);
    let _ = std::fs::remove_dir_all(root);
}
