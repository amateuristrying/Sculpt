pub mod linux;
pub mod macos;
pub mod windows;

#[allow(unused_imports)]
pub use linux::{profile_from_linux_probes, LinuxProbes};
#[allow(unused_imports)]
pub use macos::{profile_from_macos_report, MacProbes};
#[allow(unused_imports)]
pub use windows::{profile_from_windows_probes, WindowsGpuAdapter, WindowsProbes};

use serde::{Deserialize, Serialize};
use std::{process::Command, sync::OnceLock};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct HardwareProfile {
    pub os: String,
    pub os_version: String,
    pub architecture: String,
    pub chip: String,
    pub gpu: String,
    pub memory_gb: f64,
    pub unified_memory: bool,
    pub is_apple_silicon: bool,
    pub compute_backends: Vec<String>,
    pub detection_source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub storage_gb: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub npu: Option<String>,
}

impl Default for HardwareProfile {
    fn default() -> Self {
        Self {
            os: "Unknown".into(),
            os_version: "Unknown".into(),
            architecture: "Unknown".into(),
            chip: "Unknown processor".into(),
            gpu: "GPU not detected".into(),
            memory_gb: 0.0,
            unified_memory: false,
            is_apple_silicon: false,
            compute_backends: vec!["CPU".into()],
            detection_source: "native".into(),
            storage_gb: None,
            npu: None,
        }
    }
}

#[allow(dead_code)]
pub(crate) fn command_output(program: &str, args: &[&str]) -> Option<String> {
    // Fixed executables and arguments only. Never invoke a shell or accept UI input here.
    let result = Command::new(program).args(args).output().ok()?;
    result
        .status
        .success()
        .then(|| String::from_utf8_lossy(&result.stdout).trim().to_string())
}

pub fn detect() -> HardwareProfile {
    static PROFILE: OnceLock<HardwareProfile> = OnceLock::new();
    PROFILE
        .get_or_init(|| {
            if cfg!(target_os = "macos") {
                macos::detect()
            } else if cfg!(target_os = "windows") {
                windows::detect()
            } else if cfg!(target_os = "linux") {
                linux::detect()
            } else {
                HardwareProfile {
                    os: std::env::consts::OS.into(),
                    os_version: "Unknown".into(),
                    architecture: std::env::consts::ARCH.into(),
                    chip: "Unknown processor".into(),
                    gpu: "GPU not detected".into(),
                    memory_gb: 0.0,
                    unified_memory: false,
                    is_apple_silicon: false,
                    compute_backends: vec!["CPU".into()],
                    detection_source: "native".into(),
                    storage_gb: None,
                    npu: None,
                }
            }
        })
        .clone()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    #[test]
    fn detects_m4_gpu_and_metal_without_assuming_a_runtime() {
        let report = serde_json::json!({
            "SPHardwareDataType": [{"chip_type": "Apple M4", "physical_memory": "16 GB"}],
            "SPDisplaysDataType": [{"sppci_model": "Apple M4", "spdisplays_metal": "spdisplays_supported"}]
        });
        let profile = profile_from_macos_report(&report, MacProbes::default());
        assert_eq!(profile.chip, "Apple M4");
        assert!(profile.is_apple_silicon && profile.unified_memory);
        assert_eq!(profile.architecture, "arm64");
        assert_eq!(profile.memory_gb, 16.0);
        assert!(profile.compute_backends.contains(&"Metal".into()));
        assert!(!profile.compute_backends.contains(&"MLX".into()));
        assert_eq!(profile.npu, Some("Apple Neural Engine".into()));
    }

    #[test]
    fn intel_and_missing_metal_remain_honest_on_any_test_host() {
        let report = serde_json::json!({
            "SPHardwareDataType": [{"cpu_type": "Intel Core i7", "physical_memory": "8 GB"}],
            "SPDisplaysDataType": [{"sppci_model": "Intel Iris", "spdisplays_metal": "spdisplays_not_supported"}]
        });
        let profile = profile_from_macos_report(
            &report,
            MacProbes {
                architecture: Some("x86_64".into()),
                ..Default::default()
            },
        );
        assert!(!profile.is_apple_silicon && !profile.unified_memory);
        assert_eq!(profile.memory_gb, 8.0);
        assert_eq!(profile.architecture, "x86_64");
        assert_eq!(profile.compute_backends, vec!["CPU"]);
        assert_eq!(profile.npu, None);

        let missing = profile_from_macos_report(&Value::Null, MacProbes::default());
        assert_eq!(missing.chip, "Unknown processor");
        assert_eq!(missing.memory_gb, 0.0);
        assert!(!missing.is_apple_silicon);
        assert_eq!(missing.npu, None);
    }

    #[test]
    fn windows_x64_nvidia_fixture_constructs_accurate_profile() {
        let probes = WindowsProbes {
            os_name: Some("Windows 11 Pro".into()),
            display_version: Some("24H2".into()),
            build_number: Some("26100".into()),
            ubr: Some(1742),
            architecture: Some("x86_64".into()),
            cpu_name: Some("AMD Ryzen 9 9950X 16-Core Processor".into()),
            total_ram_bytes: Some(68_719_476_736), // 64 GB
            storage_total_bytes: Some(1_000_000_000_000), // 1000 GB
            storage_free_bytes: Some(500_000_000_000),
            gpus: vec![WindowsGpuAdapter {
                name: "NVIDIA GeForce RTX 4090".into(),
                provider: Some("NVIDIA".into()),
                dedicated_vram_bytes: Some(25_769_803_776), // 24 GB
                shared_ram_bytes: None,
                driver_version: Some("560.94".into()),
            }],
            npu_name: None,
            has_directml: true,
            has_vulkan: true,
            has_cuda: true,
        };

        let profile = profile_from_windows_probes(&probes);
        assert_eq!(profile.os, "Windows");
        assert_eq!(profile.os_version, "11 24H2 (Build 26100.1742)");
        assert_eq!(profile.architecture, "x86_64");
        assert_eq!(profile.chip, "AMD Ryzen 9 9950X 16-Core Processor");
        assert_eq!(profile.gpu, "NVIDIA GeForce RTX 4090");
        assert_eq!(profile.memory_gb, 64.0);
        assert!(!profile.unified_memory);
        assert!(!profile.is_apple_silicon);
        assert_eq!(
            profile.compute_backends,
            vec!["CUDA", "DirectML", "Vulkan", "CPU"]
        );
        assert_eq!(profile.storage_gb, Some(1000.0));
        assert_eq!(profile.npu, None);
    }

    #[test]
    fn windows_arm64_snapdragon_fixture_constructs_accurate_profile() {
        let probes = WindowsProbes {
            os_name: Some("Windows 11 Home".into()),
            display_version: Some("24H2".into()),
            build_number: Some("26100".into()),
            ubr: Some(1000),
            architecture: Some("arm64".into()),
            cpu_name: Some("Snapdragon(R) X Elite - X1E80100 - Qualcomm(R) Oryon(TM) CPU".into()),
            total_ram_bytes: Some(34_359_738_368), // 32 GB
            storage_total_bytes: Some(512_000_000_000),
            storage_free_bytes: Some(300_000_000_000),
            gpus: vec![WindowsGpuAdapter {
                name: "Qualcomm(R) Adreno(TM) X1-85 GPU".into(),
                provider: Some("Qualcomm Incorporated".into()),
                dedicated_vram_bytes: None, // Unified SoC memory
                shared_ram_bytes: None,
                driver_version: Some("31.0.178.0".into()),
            }],
            npu_name: Some("Snapdragon(R) X Elite - X1E80100 - Qualcomm(R) Hexagon(TM) NPU".into()),
            has_directml: true,
            has_vulkan: true,
            has_cuda: false,
        };

        let profile = profile_from_windows_probes(&probes);
        assert_eq!(profile.os, "Windows");
        assert_eq!(profile.os_version, "11 24H2 (Build 26100.1000)");
        assert_eq!(profile.architecture, "arm64");
        assert_eq!(
            profile.chip,
            "Snapdragon(R) X Elite - X1E80100 - Qualcomm(R) Oryon(TM) CPU"
        );
        assert_eq!(profile.gpu, "Qualcomm(R) Adreno(TM) X1-85 GPU");
        assert_eq!(profile.memory_gb, 32.0);
        assert!(profile.unified_memory);
        assert!(!profile.is_apple_silicon);
        assert_eq!(profile.compute_backends, vec!["DirectML", "Vulkan", "CPU"]);
        assert_eq!(profile.storage_gb, Some(512.0));
        assert_eq!(
            profile.npu,
            Some("Snapdragon(R) X Elite - X1E80100 - Qualcomm(R) Hexagon(TM) NPU".into())
        );
    }

    #[test]
    fn windows_unknown_stays_unknown_when_probes_empty() {
        let probes = WindowsProbes::default();
        let profile = profile_from_windows_probes(&probes);
        assert_eq!(profile.os, "Windows");
        assert_eq!(profile.os_version, "Unknown");
        assert_eq!(profile.architecture, "Unknown");
        assert_eq!(profile.chip, "Unknown processor");
        assert_eq!(profile.gpu, "GPU not detected");
        assert_eq!(profile.memory_gb, 0.0);
        assert!(!profile.unified_memory);
        assert_eq!(profile.compute_backends, vec!["CPU"]);
        assert_eq!(profile.storage_gb, None);
        assert_eq!(profile.npu, None);
    }

    #[test]
    fn linux_ubuntu_cuda_fixture_constructs_accurate_profile() {
        let probes = LinuxProbes {
            os_release: Some(
                r#"NAME="Ubuntu"
VERSION="22.04.4 LTS (Jammy Jellyfish)"
ID=ubuntu
PRETTY_NAME="Ubuntu 22.04.4 LTS"
VERSION_ID="22.04""#
                    .into(),
            ),
            cpuinfo: Some(
                r#"processor	: 0
vendor_id	: AuthenticAMD
model name	: AMD Ryzen 9 7950X 16-Core Processor
cpu MHz		: 4500.000"#
                    .into(),
            ),
            meminfo: Some(
                r#"MemTotal:       32645120 kB
MemFree:        24123456 kB
MemAvailable:   28901234 kB"#
                    .into(),
            ),
            df_output: Some(
                r#"Filesystem     1K-blocks      Used Available Use% Mounted on
/dev/nvme0n1p2 983040000 123456780 859583220  13% /"#
                    .into(),
            ),
            storage_total_bytes: None,
            architecture: Some("x86_64".into()),
            gpu_name: Some("NVIDIA GeForce RTX 4090".into()),
            has_nvidia: true,
            has_vulkan: true,
            is_unified_memory: false,
        };

        let profile = profile_from_linux_probes(&probes);
        assert_eq!(profile.os, "Ubuntu");
        assert_eq!(profile.os_version, "Ubuntu 22.04.4 LTS");
        assert_eq!(profile.architecture, "x86_64");
        assert_eq!(profile.chip, "AMD Ryzen 9 7950X 16-Core Processor");
        assert_eq!(profile.gpu, "NVIDIA GeForce RTX 4090");
        assert_eq!(profile.memory_gb, 31.1); // 32645120 kB -> 31.1 GB
        assert!(!profile.unified_memory);
        assert!(!profile.is_apple_silicon);
        assert_eq!(profile.compute_backends, vec!["CUDA", "Vulkan", "CPU"]);
        assert_eq!(profile.storage_gb, Some(1007.0));
        assert_eq!(profile.npu, None);
    }

    #[test]
    fn linux_cpu_only_unknown_stays_unknown_when_probes_empty() {
        let probes = LinuxProbes::default();
        let profile = profile_from_linux_probes(&probes);
        assert_eq!(profile.os, "Linux");
        assert_eq!(profile.os_version, "Unknown");
        assert_eq!(profile.architecture, "x86_64");
        assert_eq!(profile.chip, "Unknown processor");
        assert_eq!(profile.gpu, "GPU not detected");
        assert_eq!(profile.memory_gb, 0.0);
        assert!(!profile.unified_memory);
        assert_eq!(profile.compute_backends, vec!["CPU"]);
        assert_eq!(profile.storage_gb, None);
        assert_eq!(profile.npu, None);
    }

    #[test]
    fn native_detect_returns_honest_hardware_numbers() {
        let profile = detect();
        // On native platforms, detection must populate real specs instead of 0.0 GB RAM
        if cfg!(target_os = "windows") {
            assert_eq!(profile.os, "Windows");
            assert!(
                profile.memory_gb > 0.0,
                "Native Windows detect must find real physical RAM (found {})",
                profile.memory_gb
            );
            assert_ne!(
                profile.chip, "Unknown processor",
                "Native Windows detect must find CPU processor string"
            );
            assert_ne!(profile.architecture, "Unknown");
            assert!(profile.compute_backends.contains(&"CPU".into()));
            eprintln!(
                "Native Windows HardwareProfile: {}",
                serde_json::to_string_pretty(&profile).unwrap()
            );
        } else if cfg!(target_os = "macos") {
            assert_eq!(profile.os, "macOS");
            assert!(profile.compute_backends.contains(&"CPU".into()));
        } else if cfg!(target_os = "linux") {
            assert_eq!(profile.os, "Linux");
            assert!(profile.compute_backends.contains(&"CPU".into()));
        }
    }
}
