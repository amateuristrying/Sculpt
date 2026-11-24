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
