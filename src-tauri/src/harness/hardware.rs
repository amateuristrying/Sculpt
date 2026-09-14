use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{process::Command, sync::OnceLock};

#[derive(Clone, Debug, Serialize, Deserialize)]
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
}

fn command_output(program: &str, args: &[&str]) -> Option<String> {
    // Fixed executables and arguments only. Never invoke a shell or accept UI input here.
    let result = Command::new(program).args(args).output().ok()?;
    result
        .status
        .success()
        .then(|| String::from_utf8_lossy(&result.stdout).trim().to_string())
}

fn field(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)?
        .as_str()
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

#[derive(Default)]
struct MacProbes {
    cpu: Option<String>,
    arm64: bool,
    memory_bytes: Option<f64>,
    os_version: Option<String>,
    architecture: Option<String>,
}

fn profile_from_macos_report(report: &Value, probes: MacProbes) -> HardwareProfile {
    let hardware = &report["SPHardwareDataType"][0];
    let displays = report["SPDisplaysDataType"].as_array();
    let chip = field(hardware, "chip_type")
        .or(probes.cpu)
        .or_else(|| field(hardware, "cpu_type"))
        .unwrap_or_else(|| "Unknown processor".into());
    // hw.optional.arm64 remains true under Rosetta, unlike the app's build architecture.
    let is_apple_silicon = chip.starts_with("Apple M") || probes.arm64;
    let memory_gb = probes
        .memory_bytes
        .map(|bytes| bytes / 1_073_741_824.0)
        .or_else(|| {
            field(hardware, "physical_memory")
                .and_then(|value| value.split_whitespace().next()?.parse::<f64>().ok())
        })
        .unwrap_or(0.0);
    let gpu = displays
        .map(|entries| {
            entries
                .iter()
                .filter_map(|entry| field(entry, "sppci_model").or_else(|| field(entry, "_name")))
                .collect::<Vec<_>>()
                .join(" + ")
        })
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "GPU not detected".into());
    let metal = displays.is_some_and(|entries| {
        entries.iter().any(|entry| {
            // macOS uses both of these keys across releases. An absent key means unknown.
            ["spdisplays_metal", "spdisplays_metal_support"]
                .iter()
                .any(|key| {
                    entry.get(key).and_then(Value::as_str).is_some_and(|value| {
                        let value = value.to_lowercase();
                        (value.contains("supported") || value.contains("metal"))
                            && !value.contains("unsupported")
                            && !value.contains("not_supported")
                            && !value.contains("not supported")
                    })
                })
        })
    });
    let mut compute_backends = vec!["CPU".into()];
    if metal {
        compute_backends.insert(0, "Metal".into());
    }
    HardwareProfile {
        os: "macOS".into(),
        os_version: probes.os_version.unwrap_or_else(|| "Unknown".into()),
        architecture: if is_apple_silicon {
            "arm64".into()
        } else {
            probes.architecture.unwrap_or_else(|| "Unknown".into())
        },
        chip,
        gpu,
        memory_gb: (memory_gb * 10.0).round() / 10.0,
        unified_memory: is_apple_silicon,
        is_apple_silicon,
        compute_backends,
        detection_source: "native".into(),
        storage_gb: None,
    }
}

pub fn detect() -> HardwareProfile {
    static PROFILE: OnceLock<HardwareProfile> = OnceLock::new();
    PROFILE
        .get_or_init(|| {
            if cfg!(target_os = "macos") {
                let report = command_output(
                    "/usr/sbin/system_profiler",
                    &[
                        "SPHardwareDataType",
                        "SPDisplaysDataType",
                        "-detailLevel",
                        "mini",
                        "-timeout",
                        "8",
                        "-json",
                    ],
                )
                .and_then(|value| serde_json::from_str::<Value>(&value).ok())
                .unwrap_or(Value::Null);
                let probes = MacProbes {
                    cpu: command_output("/usr/sbin/sysctl", &["-n", "machdep.cpu.brand_string"]),
                    arm64: command_output("/usr/sbin/sysctl", &["-n", "hw.optional.arm64"])
                        .as_deref()
                        == Some("1"),
                    memory_bytes: command_output("/usr/sbin/sysctl", &["-n", "hw.memsize"])
                        .and_then(|value| value.parse::<f64>().ok()),
                    os_version: command_output("/usr/bin/sw_vers", &["-productVersion"]),
                    architecture: Some(std::env::consts::ARCH.into()),
                };
                let mut profile = profile_from_macos_report(&report, probes);
                // Disk Size is the root volume/container capacity; report only when detected.
                profile.storage_gb = command_output("/usr/sbin/diskutil", &["info", "/"])
                    .and_then(|output| {
                        output
                            .lines()
                            .find(|line| line.trim_start().starts_with("Disk Size:"))
                            .map(str::to_owned)
                    })
                    .and_then(|line| {
                        line.split('(')
                            .nth(1)?
                            .split_whitespace()
                            .next()?
                            .parse::<f64>()
                            .ok()
                    })
                    .map(|bytes| (bytes / 1_000_000_000.0).round());
                profile
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
                }
            }
        })
        .clone()
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let missing = profile_from_macos_report(&Value::Null, MacProbes::default());
        assert_eq!(missing.chip, "Unknown processor");
        assert_eq!(missing.memory_gb, 0.0);
        assert!(!missing.is_apple_silicon);
    }

    #[test]
    #[ignore = "Runs hardware utilities on this Mac; use --ignored --nocapture for native verification"]
    fn inspect_development_hardware() {
        println!("{}", serde_json::to_string_pretty(&detect()).unwrap());
    }
}
