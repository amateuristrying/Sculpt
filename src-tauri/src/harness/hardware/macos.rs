use super::HardwareProfile;
use serde::{Deserialize, Serialize};
use serde_json::Value;

fn field(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)?
        .as_str()
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

#[derive(Default, Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MacProbes {
    pub cpu: Option<String>,
    pub arm64: bool,
    pub memory_bytes: Option<f64>,
    pub os_version: Option<String>,
    pub architecture: Option<String>,
}

pub fn profile_from_macos_report(report: &Value, probes: MacProbes) -> HardwareProfile {
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
        npu: if is_apple_silicon {
            Some("Apple Neural Engine".into())
        } else {
            None
        },
    }
}

#[cfg(target_os = "macos")]
pub fn detect() -> HardwareProfile {
    let report = super::command_output(
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
        cpu: super::command_output("/usr/sbin/sysctl", &["-n", "machdep.cpu.brand_string"]),
        arm64: super::command_output("/usr/sbin/sysctl", &["-n", "hw.optional.arm64"]).as_deref()
            == Some("1"),
        memory_bytes: super::command_output("/usr/sbin/sysctl", &["-n", "hw.memsize"])
            .and_then(|value| value.parse::<f64>().ok()),
        os_version: super::command_output("/usr/bin/sw_vers", &["-productVersion"]),
        architecture: Some(std::env::consts::ARCH.into()),
    };
    let mut profile = profile_from_macos_report(&report, probes);
    // Disk Size is the root volume/container capacity; report only when detected.
    profile.storage_gb = super::command_output("/usr/sbin/diskutil", &["info", "/"])
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
}

#[cfg(not(target_os = "macos"))]
pub fn detect() -> HardwareProfile {
    profile_from_macos_report(&Value::Null, MacProbes::default())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_apple_silicon_m_series_variants() {
        for chip in &["Apple M1", "Apple M2 Pro", "Apple M3 Max", "Apple M4"] {
            let report = json!({
                "SPHardwareDataType": [{"chip_type": chip, "physical_memory": "32 GB"}],
                "SPDisplaysDataType": [{"sppci_model": chip, "spdisplays_metal": "spdisplays_supported"}]
            });
            let profile = profile_from_macos_report(
                &report,
                MacProbes {
                    cpu: Some((*chip).into()),
                    arm64: true,
                    memory_bytes: Some(34_359_738_368.0),
                    os_version: Some("15.1".into()),
                    architecture: Some("arm64".into()),
                },
            );
            assert_eq!(profile.os, "macOS");
            assert_eq!(profile.chip, *chip);
            assert!(profile.is_apple_silicon);
            assert!(profile.unified_memory);
            assert_eq!(profile.architecture, "arm64");
            assert_eq!(profile.memory_gb, 32.0);
            assert_eq!(profile.compute_backends, vec!["Metal", "CPU"]);
            assert_eq!(profile.npu, Some("Apple Neural Engine".into()));
        }
    }

    #[test]
    fn test_rosetta_arm64_probe_preserves_apple_silicon() {
        // Under Rosetta translation, host sysctl hw.optional.arm64 is true (1),
        // but architecture probe might report x86_64 or translated binary
        let report = json!({
            "SPHardwareDataType": [{"chip_type": "Apple M3", "physical_memory": "16 GB"}],
            "SPDisplaysDataType": [{"sppci_model": "Apple M3", "spdisplays_metal": "Metal Supported"}]
        });
        let profile = profile_from_macos_report(
            &report,
            MacProbes {
                cpu: Some("Apple M3".into()),
                arm64: true,
                memory_bytes: Some(17_179_869_184.0),
                os_version: Some("14.4".into()),
                architecture: Some("x86_64".into()), // Translated binary architecture
            },
        );
        assert!(profile.is_apple_silicon);
        assert!(profile.unified_memory);
        // Architecture must still be normalized to arm64 because it is physical Apple Silicon
        assert_eq!(profile.architecture, "arm64");
        assert_eq!(profile.npu, Some("Apple Neural Engine".into()));
    }

    #[test]
    fn test_metal_support_string_variations() {
        // Positive keys: spdisplays_metal and spdisplays_metal_support with various valid values
        for val in &[
            ("spdisplays_metal", "spdisplays_supported"),
            ("spdisplays_metal", "Metal 3"),
            ("spdisplays_metal_support", "Metal Supported"),
            (
                "spdisplays_metal_support",
                "Metal Feature Set macOS GPUFamily2 v1",
            ),
        ] {
            let report = json!({
                "SPHardwareDataType": [{"chip_type": "Apple M2", "physical_memory": "16 GB"}],
                "SPDisplaysDataType": [{"sppci_model": "Apple M2", val.0: val.1}]
            });
            let profile = profile_from_macos_report(&report, MacProbes::default());
            assert!(
                profile.compute_backends.contains(&"Metal".into()),
                "Failed to detect Metal for key {} with value {}",
                val.0,
                val.1
            );
        }

        // Negative values: unsupported
        for neg in &["spdisplays_not_supported", "unsupported", "not supported"] {
            let report = json!({
                "SPHardwareDataType": [{"chip_type": "Apple M2", "physical_memory": "16 GB"}],
                "SPDisplaysDataType": [{"sppci_model": "Apple M2", "spdisplays_metal": neg}]
            });
            let profile = profile_from_macos_report(&report, MacProbes::default());
            assert_eq!(
                profile.compute_backends,
                vec!["CPU"],
                "Expected CPU only for negative Metal value '{}'",
                neg
            );
        }
    }

    #[test]
    fn test_multi_display_concatenation() {
        let report = json!({
            "SPHardwareDataType": [{"chip_type": "Apple M1 Max", "physical_memory": "64 GB"}],
            "SPDisplaysDataType": [
                {"sppci_model": "Apple M1 Max", "spdisplays_metal": "spdisplays_supported"},
                {"_name": "Studio Display", "spdisplays_metal": "spdisplays_supported"}
            ]
        });
        let profile = profile_from_macos_report(&report, MacProbes::default());
        assert_eq!(profile.gpu, "Apple M1 Max + Studio Display");
    }

    #[test]
    fn test_sysctl_memory_bytes_preferred_over_physical_memory_string() {
        let report = json!({
            "SPHardwareDataType": [{"chip_type": "Apple M4", "physical_memory": "16 GB"}],
            "SPDisplaysDataType": [{"sppci_model": "Apple M4", "spdisplays_metal": "spdisplays_supported"}]
        });
        // 24 GB via sysctl bytes overrides the 16 GB coarse string in report
        let profile = profile_from_macos_report(
            &report,
            MacProbes {
                memory_bytes: Some(25_769_803_776.0),
                ..Default::default()
            },
        );
        assert_eq!(profile.memory_gb, 24.0);
    }
}
