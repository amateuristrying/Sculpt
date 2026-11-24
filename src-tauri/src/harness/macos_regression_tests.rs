//! macOS regression tests through portable interfaces.
//!
//! Confirms that moving host services behind portable cross-platform contracts
//! (hardware profiles, process supervision, file locking, path resolution,
//! runtime setup) 100% preserves Apple Silicon (M-series, unified memory, Metal/MPS)
//! baseline behaviour without regression.

use crate::harness::{
    catalog::engines,
    engines::{
        evaluate_manifest, manifests, resolve, CompatibilityStatus, ComputeDevice, RuntimeAdapter,
    },
    hardware::macos::{profile_from_macos_report, MacProbes},
    paths, python, setup,
};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{fs, path::Path, time::UNIX_EPOCH};

/// Constructs an authoritative Apple Silicon M4 Pro hardware fixture.
fn m4_pro_fixture() -> (Value, MacProbes) {
    let report = serde_json::json!({
        "SPHardwareDataType": [
            {
                "chip_type": "Apple M4 Pro",
                "physical_memory": "24 GB",
                "number_processors": "proc 12:8:4",
                "platform_UUID": "00000000-0000-0000-0000-000000000000"
            }
        ],
        "SPDisplaysDataType": [
            {
                "_name": "Color LCD",
                "sppci_model": "Apple M4 Pro",
                "spdisplays_metal": "spdisplays_supported"
            }
        ]
    });
    let probes = MacProbes {
        cpu: Some("Apple M4 Pro".into()),
        arm64: true,
        memory_bytes: Some(25_769_803_776.0), // 24 GB
        os_version: Some("15.2".into()),
        architecture: Some("arm64".into()),
    };
    (report, probes)
}

#[test]
fn macos_hardware_profile_and_engine_recommendation_end_to_end() {
    let (report, probes) = m4_pro_fixture();
    let profile = profile_from_macos_report(&report, probes);

    // 1. Verify HardwareProfile fields for Apple Silicon
    assert_eq!(profile.os, "macOS");
    assert_eq!(profile.os_version, "15.2");
    assert_eq!(profile.architecture, "arm64");
    assert_eq!(profile.chip, "Apple M4 Pro");
    assert_eq!(profile.gpu, "Apple M4 Pro");
    assert_eq!(profile.memory_gb, 24.0);
    assert!(profile.unified_memory);
    assert!(profile.is_apple_silicon);
    assert_eq!(profile.compute_backends, vec!["Metal", "CPU"]);
    assert_eq!(profile.detection_source, "native");
    assert_eq!(profile.npu, Some("Apple Neural Engine".into()));

    // 2. Hardware preflight check passes on macOS Apple Silicon with 24 GB RAM
    assert!(setup::validate_setup_hardware(&profile).is_ok());

    // 3. Engine resolution for TripoSR resolves the validated MPS adapter
    let selection = resolve("triposr", &profile).expect("TripoSR must resolve on Apple Silicon M4");
    assert_eq!(selection.adapter, RuntimeAdapter::TripoSr);
    assert_eq!(selection.device, ComputeDevice::Mps);
    assert_eq!(selection.device.as_str(), "mps");
    assert_eq!(selection.manifest.id, "triposr");
    assert!(selection.manifest.capabilities.cancellation);
    assert!(selection.manifest.capabilities.refinement);
    assert!(selection.manifest.capabilities.progress_reporting);

    // 4. Engine catalog marks TripoSR as recommended with Metal acceleration
    let catalog = engines(&profile);
    assert!(!catalog.is_empty());

    let triposr_entry = catalog
        .iter()
        .find(|e| e.id == "triposr")
        .expect("TripoSR must be present in catalog");
    assert_eq!(triposr_entry.compatibility, "recommended");
    assert_eq!(
        triposr_entry.compatibility_status,
        CompatibilityStatus::Supported
    );
    assert!(triposr_entry.implemented);
    assert_eq!(triposr_entry.runtime, "PyTorch / Metal");
    assert!(!triposr_entry.requires_confirmation);
    assert!(!triposr_entry.requires_cpu_confirmation);
    assert!(triposr_entry.resource_preflight.passed);
    assert!(triposr_entry.resource_preflight.ram_passed);
    assert_eq!(triposr_entry.resource_preflight.ram_detected_gb, 24.0);
    assert_eq!(triposr_entry.resource_preflight.ram_minimum_gb, Some(16.0));
    assert_eq!(
        triposr_entry.recommendation_reason,
        Some("Validated for Metal acceleration.".into())
    );

    // 5. Procedural demo engine remains available as alternative, not recommended
    let demo_entry = catalog
        .iter()
        .find(|e| e.id == "demo")
        .expect("Demo engine must be present in catalog");
    assert_eq!(demo_entry.compatibility, "available");
    assert_eq!(
        demo_entry.compatibility_status,
        CompatibilityStatus::Supported
    );
}

#[test]
fn macos_low_ram_apple_silicon_reports_insufficient_resources_and_recommends_demo() {
    let report = serde_json::json!({
        "SPHardwareDataType": [
            {
                "chip_type": "Apple M1",
                "physical_memory": "8 GB"
            }
        ],
        "SPDisplaysDataType": [
            {
                "sppci_model": "Apple M1",
                "spdisplays_metal": "spdisplays_supported"
            }
        ]
    });
    let probes = MacProbes {
        cpu: Some("Apple M1".into()),
        arm64: true,
        memory_bytes: Some(8_589_934_592.0), // 8 GB
        os_version: Some("14.5".into()),
        architecture: Some("arm64".into()),
    };
    let profile = profile_from_macos_report(&report, probes);

    assert_eq!(profile.memory_gb, 8.0);
    assert!(profile.is_apple_silicon);
    assert!(profile.compute_backends.contains(&"Metal".into()));

    // Runtime setup preflight allows 8 GB system RAM
    assert!(setup::validate_setup_hardware(&profile).is_ok());

    // But TripoSR manifest requires minimum 16 GB RAM admission threshold
    let triposr_manifest = &manifests()[0];
    let evaluation = evaluate_manifest(&profile, triposr_manifest);
    assert_eq!(evaluation.compatibility, "unsupported");
    assert_eq!(
        evaluation.compatibility_status,
        CompatibilityStatus::InsufficientResources
    );
    assert!(!evaluation.resource_preflight.passed);
    assert!(!evaluation.resource_preflight.ram_passed);
    assert_eq!(evaluation.resource_preflight.ram_detected_gb, 8.0);
    assert_eq!(evaluation.resource_preflight.ram_minimum_gb, Some(16.0));
    assert!(evaluation.reason.contains("at least 16 GB"));

    // Resolver rejects TripoSR on 8 GB Mac
    let err = resolve("triposr", &profile).unwrap_err();
    assert!(err.contains("at least 16 GB"));

    // In catalog, TripoSR is unsupported and demo becomes recommended fallback
    let catalog = engines(&profile);
    let triposr_entry = catalog.iter().find(|e| e.id == "triposr").unwrap();
    assert_eq!(triposr_entry.compatibility, "unsupported");
    assert_eq!(
        triposr_entry.compatibility_status,
        CompatibilityStatus::InsufficientResources
    );

    let demo_entry = catalog.iter().find(|e| e.id == "demo").unwrap();
    assert_eq!(demo_entry.compatibility, "recommended");
    assert_eq!(
        demo_entry.recommendation_reason,
        Some("Recommended zero-dependency fallback workspace demo.".into())
    );
}

#[test]
fn macos_missing_metal_reports_unsupported_and_never_falls_back_to_cpu() {
    let report = serde_json::json!({
        "SPHardwareDataType": [
            {
                "chip_type": "Apple M2",
                "physical_memory": "16 GB"
            }
        ],
        "SPDisplaysDataType": [
            {
                "sppci_model": "Apple M2 Display",
                "spdisplays_metal": "spdisplays_not_supported"
            }
        ]
    });
    let probes = MacProbes {
        cpu: Some("Apple M2".into()),
        arm64: true,
        memory_bytes: Some(17_179_869_184.0), // 16 GB
        os_version: Some("14.0".into()),
        architecture: Some("arm64".into()),
    };
    let profile = profile_from_macos_report(&report, probes);

    // Profile honestly reflects missing Metal
    assert_eq!(profile.compute_backends, vec!["CPU"]);
    assert!(!profile.compute_backends.contains(&"Metal".into()));

    // TripoSR evaluation fails because Metal backend is missing; CPU is unsupported
    let triposr_manifest = &manifests()[0];
    let evaluation = evaluate_manifest(&profile, triposr_manifest);
    assert_eq!(evaluation.compatibility, "unsupported");
    assert_eq!(
        evaluation.compatibility_status,
        CompatibilityStatus::Unsupported
    );
    assert!(evaluation
        .reason
        .contains("compute backend required by this validated runtime was not detected"));

    // Resolver rejects TripoSR without silently falling back to CPU
    let err = resolve("triposr", &profile).unwrap_err();
    assert!(err.contains("compute backend required by this validated runtime was not detected"));

    // In catalog, Demo is recommended because TripoSR is unavailable
    let catalog = engines(&profile);
    let triposr_entry = catalog.iter().find(|e| e.id == "triposr").unwrap();
    assert_eq!(triposr_entry.compatibility, "unsupported");
    let demo_entry = catalog.iter().find(|e| e.id == "demo").unwrap();
    assert_eq!(demo_entry.compatibility, "recommended");
}

#[test]
fn macos_intel_mac_rejected_by_hardware_preflight_and_catalog() {
    let report = serde_json::json!({
        "SPHardwareDataType": [
            {
                "cpu_type": "Intel Core i9",
                "physical_memory": "32 GB"
            }
        ],
        "SPDisplaysDataType": [
            {
                "sppci_model": "AMD Radeon Pro 5500M",
                "spdisplays_metal": "Metal Supported"
            }
        ]
    });
    let probes = MacProbes {
        cpu: Some("Intel(R) Core(TM) i9-9880H CPU @ 2.30GHz".into()),
        arm64: false,
        memory_bytes: Some(34_359_738_368.0), // 32 GB
        os_version: Some("13.6".into()),
        architecture: Some("x86_64".into()),
    };
    let profile = profile_from_macos_report(&report, probes);

    assert_eq!(profile.os, "macOS");
    assert_eq!(profile.architecture, "x86_64");
    assert!(!profile.is_apple_silicon);
    assert!(!profile.unified_memory);
    assert_eq!(profile.npu, None);

    // Setup hardware preflight strictly enforces Apple Silicon on macOS
    let err = setup::validate_setup_hardware(&profile).unwrap_err();
    assert!(err.contains("requires an Apple Silicon Mac"));

    // TripoSR requires Apple Silicon
    let err = resolve("triposr", &profile).unwrap_err();
    assert!(err.contains("no validated Sculpt runtime"));

    // Catalog marks TripoSR unsupported
    let catalog = engines(&profile);
    assert_eq!(catalog[0].compatibility, "unsupported");
    assert_eq!(
        catalog[0].compatibility_status,
        CompatibilityStatus::Unsupported
    );
}

#[test]
fn macos_paths_and_manifest_conformance() {
    let dummy_root = Path::new("/Users/sculpt/Library/Application Support/Sculpt/runtime");

    // 1. Python executable resolver on Unix/macOS resolves venv/bin/python
    let macos_py = paths::python_executable_for_platform(dummy_root, false);
    assert_eq!(macos_py, dummy_root.join("venv").join("bin").join("python"));

    // 2. Centralized application directories
    assert_eq!(
        paths::cache_dir(dummy_root),
        dummy_root.join("package-cache")
    );
    assert_eq!(paths::models_dir(dummy_root), dummy_root.join("models"));
    let library_root = Path::new("/Users/sculpt/Library/Application Support/Sculpt/library");
    assert_eq!(paths::jobs_dir(library_root), library_root.join("jobs"));

    // 3. Official pinned uv release resolution for macOS
    let mac_arm = setup::uv_release_for("macos", "aarch64").expect("macOS ARM64 uv release");
    assert_eq!(
        mac_arm.url,
        "https://github.com/astral-sh/uv/releases/download/0.10.10/uv-aarch64-apple-darwin.tar.gz"
    );
    assert_eq!(
        mac_arm.sha256,
        "8a09f0ef51ee7f7170731b4cb8bde5bf9ba6da5304f49a7df6cdab42a1f37b5d"
    );
    assert_eq!(mac_arm.archive_filename, "uv-aarch64-apple-darwin.tar.gz");
    assert_eq!(mac_arm.binary_name, "uv");
    assert!(!mac_arm.is_zip);
    assert_eq!(
        mac_arm.archive_member_prefix,
        Some("uv-aarch64-apple-darwin")
    );

    // Darwin alias resolution
    let darwin_arm = setup::uv_release_for("darwin", "arm64").expect("Darwin ARM64 uv release");
    assert_eq!(darwin_arm, mac_arm);

    // macOS Intel release resolution
    let mac_intel = setup::uv_release_for("macos", "x86_64").expect("macOS x86_64 uv release");
    assert_eq!(
        mac_intel.url,
        "https://github.com/astral-sh/uv/releases/download/0.10.10/uv-x86_64-apple-darwin.tar.gz"
    );
    assert_eq!(
        mac_intel.sha256,
        "dd18420591d625f9b4ca2b57a7a6fe3cce43910f02e02d90e47a4101428de14a"
    );
    assert_eq!(mac_intel.archive_filename, "uv-x86_64-apple-darwin.tar.gz");
    assert_eq!(mac_intel.binary_name, "uv");
    assert!(!mac_intel.is_zip);
}

#[test]
fn macos_simulated_runtime_inspect_validates_mps_and_ready_state() {
    let temp_dir = std::env::temp_dir().join(format!(
        "sculpt_test_macos_runtime_{}",
        uuid::Uuid::new_v4()
    ));
    fs::create_dir_all(temp_dir.join("venv/bin")).unwrap();
    fs::create_dir_all(temp_dir.join("TripoSR/tsr")).unwrap();
    fs::create_dir_all(temp_dir.join("models/triposr")).unwrap();
    fs::create_dir_all(temp_dir.join("models/background")).unwrap();

    let files_to_create = [
        "venv/bin/python",
        "TripoSR/tsr/system.py",
        "models/triposr/config.yaml",
        "models/triposr/model.ckpt",
        "models/background/u2net.onnx",
    ];

    let mut file_entries = serde_json::Map::new();
    for rel_path in &files_to_create {
        let full = temp_dir.join(rel_path);
        fs::write(&full, b"verified content").unwrap();
        let stat = fs::metadata(&full).unwrap();
        let modified = stat
            .modified()
            .unwrap()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos() as u64;
        file_entries.insert(
            (*rel_path).to_string(),
            serde_json::json!({
                "size": stat.len(),
                "modifiedNs": modified,
            }),
        );
    }

    let spec: Value = serde_json::from_str(include_str!("../../../backend/runtime-spec.json"))
        .expect("Bundled runtime specification");
    let lock_sha = format!(
        "{:x}",
        <Sha256 as Digest>::digest(include_bytes!("../../../backend/requirements.lock"))
    );

    let manifest = serde_json::json!({
        "version": spec["version"],
        "engine": "triposr",
        "modelRevision": spec["modelRevision"],
        "sourceRevision": spec["sourceRevision"],
        "mpsAvailable": true,
        "cudaAvailable": false,
        "files": file_entries,
        "dependencyLockSha256": lock_sha,
    });

    fs::write(
        temp_dir.join("ready.json"),
        serde_json::to_vec_pretty(&manifest).unwrap(),
    )
    .unwrap();

    let status = python::inspect(&temp_dir);
    assert!(
        status.installed,
        "Simulated macOS runtime must pass verification"
    );
    assert!(
        status.mps_available,
        "mpsAvailable must be true when recorded in ready.json"
    );
    assert_eq!(status.state, "ready");
    assert_eq!(status.engine, "TripoSR");
    assert_eq!(status.runtime_path, temp_dir.to_string_lossy());

    let _ = fs::remove_dir_all(temp_dir);
}

#[test]
fn macos_archive_traversal_protection() {
    // Valid macOS archive entries extracted with --strip-components 1
    assert!(setup::validate_archive_entries(&[
        "uv-aarch64-apple-darwin/",
        "uv-aarch64-apple-darwin/uv",
    ])
    .is_ok());

    // Malicious archive trying to escape to root or home
    assert!(setup::validate_archive_entries(&["../Library/Application Support/evil"]).is_err());
    assert!(setup::validate_archive_entries(&["/usr/local/bin/evil"]).is_err());
    assert!(setup::validate_archive_entries(&["uv-aarch64-apple-darwin/../../bin/evil"]).is_err());
}

#[test]
fn macos_tauri_bundle_and_capabilities_are_valid() {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let base_conf: Value =
        serde_json::from_str(&fs::read_to_string(manifest_dir.join("tauri.conf.json")).unwrap())
            .expect("valid tauri.conf.json");
    let macos_conf: Value = serde_json::from_str(
        &fs::read_to_string(manifest_dir.join("tauri.macos.conf.json")).unwrap(),
    )
    .expect("valid tauri.macos.conf.json");

    assert_eq!(
        base_conf["bundle"]["targets"],
        serde_json::json!(["app", "dmg"])
    );
    assert_eq!(
        macos_conf["bundle"]["targets"],
        serde_json::json!(["app", "dmg"])
    );
    assert_eq!(
        macos_conf["bundle"]["macOS"]["minimumSystemVersion"],
        "12.0"
    );
    assert_eq!(macos_conf["bundle"]["macOS"]["signingIdentity"], "-");

    let icons = macos_conf["bundle"]["icon"]
        .as_array()
        .expect("macOS bundle icon array");
    assert!(icons.iter().any(|icon| icon == "icons/icon.icns"));
    for icon in icons {
        let rel = icon.as_str().expect("icon path string");
        assert!(
            manifest_dir.join(rel).is_file(),
            "Missing macOS icon file: {rel}"
        );
    }

    let cap: Value = serde_json::from_str(
        &fs::read_to_string(manifest_dir.join("capabilities/default.json")).unwrap(),
    )
    .expect("valid capabilities/default.json");
    let perms = cap["permissions"].as_array().unwrap();
    assert!(perms.iter().any(|p| p == "core:default"));
    assert!(perms
        .iter()
        .any(|p| p == "core:window:allow-start-dragging"));
    assert!(cap.get("platforms").is_none());
}
