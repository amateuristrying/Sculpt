use super::HardwareProfile;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LinuxProbes {
    pub os_release: Option<String>,
    pub cpuinfo: Option<String>,
    pub meminfo: Option<String>,
    pub df_output: Option<String>,
    pub storage_total_bytes: Option<u64>,
    pub architecture: Option<String>,
    pub gpu_name: Option<String>,
    pub has_nvidia: bool,
    pub has_vulkan: bool,
    pub is_unified_memory: bool,
}

pub fn profile_from_linux_probes(probes: &LinuxProbes) -> HardwareProfile {
    // 1. Architecture: normalize to "arm64" or "x86_64"
    let architecture = match probes.architecture.as_deref() {
        Some("aarch64") | Some("arm64") => "arm64".to_string(),
        Some("x86_64") | Some("amd64") => "x86_64".to_string(),
        Some(other) => other.to_string(),
        None => "x86_64".to_string(),
    };

    // 2. OS and OS version from /etc/os-release
    let (os, os_version) = if let Some(ref content) = probes.os_release {
        let mut pretty_name = None;
        let mut name = None;
        let mut version_id = None;
        for line in content.lines() {
            let line = line.trim();
            if let Some((k, v)) = line.split_once('=') {
                let val = v.trim().trim_matches('"').trim_matches('\'');
                match k.trim() {
                    "PRETTY_NAME" => pretty_name = Some(val.to_string()),
                    "NAME" => name = Some(val.to_string()),
                    "VERSION_ID" => version_id = Some(val.to_string()),
                    _ => {}
                }
            }
        }
        let os = name.unwrap_or_else(|| "Linux".to_string());
        let version = pretty_name
            .or(version_id)
            .unwrap_or_else(|| "Unknown".to_string());
        (os, version)
    } else {
        ("Linux".to_string(), "Unknown".to_string())
    };

    // 3. CPU from /proc/cpuinfo: prefer "model name", then "hardware", then non-numeric "processor"
    let chip = if let Some(ref content) = probes.cpuinfo {
        let mut model_name = None;
        let mut fallback = None;
        for line in content.lines() {
            let line = line.trim();
            if let Some((k, v)) = line.split_once(':') {
                let key = k.trim().to_lowercase();
                let val = v.trim();
                if val.is_empty() {
                    continue;
                }
                if key == "model name" {
                    model_name = Some(val.to_string());
                    break;
                } else if (key == "hardware" || key == "model" || key == "processor")
                    && !val.chars().all(|c| c.is_ascii_digit())
                    && fallback.is_none()
                {
                    fallback = Some(val.to_string());
                }
            }
        }
        model_name
            .or(fallback)
            .unwrap_or_else(|| "Unknown processor".into())
    } else {
        "Unknown processor".into()
    };

    // 4. Memory from /proc/meminfo
    let memory_gb = if let Some(ref content) = probes.meminfo {
        content
            .lines()
            .find_map(|line| {
                let line = line.trim();
                if line.starts_with("MemTotal:") {
                    line.split_whitespace()
                        .nth(1)
                        .and_then(|kb_str| kb_str.parse::<f64>().ok())
                        .map(|kb| {
                            let bytes = kb * 1024.0;
                            ((bytes / 1_073_741_824.0) * 10.0).round() / 10.0
                        })
                } else {
                    None
                }
            })
            .unwrap_or(0.0)
    } else {
        0.0
    };

    // 5. Storage: capacity in GB
    let storage_gb = probes
        .storage_total_bytes
        .map(|bytes| (bytes as f64 / 1_000_000_000.0).round())
        .or_else(|| {
            probes.df_output.as_ref().and_then(|df| {
                let lines: Vec<&str> = df.lines().collect();
                if lines.len() >= 2 {
                    let cols: Vec<&str> = lines[1].split_whitespace().collect();
                    if cols.len() >= 2 {
                        let kb = cols[1].parse::<f64>().ok()?;
                        Some(((kb * 1024.0) / 1_000_000_000.0).round())
                    } else {
                        None
                    }
                } else {
                    None
                }
            })
        });

    // 6. GPU
    let gpu = probes
        .gpu_name
        .clone()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "GPU not detected".into());

    // 7. Compute backends: CUDA, Vulkan, CPU
    let mut compute_backends = Vec::new();
    if probes.has_nvidia {
        compute_backends.push("CUDA".into());
    }
    if probes.has_vulkan {
        compute_backends.push("Vulkan".into());
    }
    compute_backends.push("CPU".into());

    HardwareProfile {
        os,
        os_version,
        architecture,
        chip,
        gpu,
        memory_gb,
        unified_memory: probes.is_unified_memory,
        is_apple_silicon: false,
        compute_backends,
        detection_source: "native".into(),
        storage_gb,
        npu: None,
    }
}

#[cfg(target_os = "linux")]
pub fn native_linux_probes() -> LinuxProbes {
    let mut probes = LinuxProbes::default();
    probes.os_release = std::fs::read_to_string("/etc/os-release").ok();
    probes.cpuinfo = std::fs::read_to_string("/proc/cpuinfo").ok();
    probes.meminfo = std::fs::read_to_string("/proc/meminfo").ok();
    probes.architecture = Some(std::env::consts::ARCH.into());

    // Storage via df -k /
    probes.df_output = super::command_output("df", &["-k", "/"]);

    // GPU / NVIDIA check via nvidia-smi or driver presence
    if let Some(gpu_out) =
        super::command_output("nvidia-smi", &["--query-gpu=name", "--format=csv,noheader"])
    {
        let first_line = gpu_out.lines().next().unwrap_or("").trim();
        if !first_line.is_empty() {
            probes.has_nvidia = true;
            probes.gpu_name = Some(first_line.to_string());
        }
    } else if std::path::Path::new("/proc/driver/nvidia/version").exists() {
        probes.has_nvidia = true;
        probes.gpu_name = Some("NVIDIA GPU".into());
    }

    // Vulkan check via ICD definitions or shared library
    let vulkan_icd = std::path::Path::new("/usr/share/vulkan/icd.d");
    let has_icd = vulkan_icd.is_dir()
        && std::fs::read_dir(vulkan_icd)
            .map(|mut r| r.next().is_some())
            .unwrap_or(false);
    let has_vulkan_lib = std::path::Path::new("/usr/lib/x86_64-linux-gnu/libvulkan.so.1").exists()
        || std::path::Path::new("/usr/lib/libvulkan.so.1").exists();
    probes.has_vulkan = has_icd || has_vulkan_lib;

    // Unified memory determination: ARM64 without discrete GPU uses unified RAM
    probes.is_unified_memory = std::env::consts::ARCH == "aarch64" && !probes.has_nvidia;

    probes
}

#[cfg(not(target_os = "linux"))]
pub fn native_linux_probes() -> LinuxProbes {
    LinuxProbes::default()
}

pub fn detect() -> HardwareProfile {
    profile_from_linux_probes(&native_linux_probes())
}
