use super::HardwareProfile;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct WindowsGpuAdapter {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dedicated_vram_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shared_ram_bytes: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub driver_version: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowsProbes {
    pub os_name: Option<String>,
    pub display_version: Option<String>,
    pub build_number: Option<String>,
    pub ubr: Option<u32>,
    pub architecture: Option<String>,
    pub cpu_name: Option<String>,
    pub total_ram_bytes: Option<u64>,
    pub storage_total_bytes: Option<u64>,
    pub storage_free_bytes: Option<u64>,
    pub gpus: Vec<WindowsGpuAdapter>,
    pub npu_name: Option<String>,
    pub has_directml: bool,
    pub has_vulkan: bool,
    pub has_cuda: bool,
}

pub fn profile_from_windows_probes(probes: &WindowsProbes) -> HardwareProfile {
    // 1. Architecture: normalize to "arm64" or "x86_64"
    let architecture = match probes.architecture.as_deref() {
        Some("aarch64") | Some("arm64") | Some("ARM64") => "arm64".to_string(),
        Some("x86_64") | Some("AMD64") | Some("amd64") => "x86_64".to_string(),
        Some(other) => other.to_string(),
        None => "Unknown".to_string(),
    };

    // 2. OS version
    let os_version = if let Some(ref build) = probes.build_number {
        let build_num = build.parse::<u32>().unwrap_or(0);
        let major = if build_num >= 22000 {
            "11"
        } else if build_num >= 10240 {
            "10"
        } else {
            probes.os_name.as_deref().unwrap_or("Windows")
        };
        let ubr_suffix = probes.ubr.map(|u| format!(".{}", u)).unwrap_or_default();
        if let Some(ref dv) = probes.display_version {
            format!("{} {} (Build {}{})", major, dv, build, ubr_suffix)
        } else {
            format!("{} (Build {}{})", major, build, ubr_suffix)
        }
    } else {
        probes.os_name.clone().unwrap_or_else(|| "Unknown".into())
    };

    // 3. CPU
    let chip = probes
        .cpu_name
        .clone()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "Unknown processor".into());

    // 4. GPUs & unified memory
    let non_software_gpus: Vec<&WindowsGpuAdapter> = probes
        .gpus
        .iter()
        .filter(|g| {
            let n = g.name.to_lowercase();
            !n.contains("basic display adapter") && !n.contains("remote display adapter")
        })
        .collect();

    let effective_gpus = if !non_software_gpus.is_empty() {
        non_software_gpus
    } else {
        probes.gpus.iter().collect()
    };

    let gpu = if !effective_gpus.is_empty() {
        effective_gpus
            .iter()
            .map(|g| g.name.clone())
            .collect::<Vec<_>>()
            .join(" + ")
    } else {
        "GPU not detected".into()
    };

    // Unified memory determination:
    // ARM64 SoCs (e.g. Qualcomm Snapdragon X) integrate CPU, Adreno GPU, and NPU sharing unified system memory.
    // Discrete GPUs with dedicated VRAM (> 512 MB) have non-unified memory.
    let has_discrete_vram = effective_gpus.iter().any(|g| {
        g.dedicated_vram_bytes
            .is_some_and(|b| b > 512 * 1024 * 1024)
    });
    let unified_memory = if architecture == "arm64" {
        true
    } else {
        !has_discrete_vram && !effective_gpus.is_empty()
    };

    // 5. RAM in GB rounded to 1 decimal place
    let memory_gb = probes
        .total_ram_bytes
        .map(|bytes| ((bytes as f64 / 1_073_741_824.0) * 10.0).round() / 10.0)
        .unwrap_or(0.0);

    // 6. Compute Backends: CUDA, DirectML, Vulkan, CPU
    let mut compute_backends = Vec::new();
    if probes.has_cuda {
        compute_backends.push("CUDA".into());
    }
    if probes.has_directml {
        compute_backends.push("DirectML".into());
    }
    if probes.has_vulkan {
        compute_backends.push("Vulkan".into());
    }
    compute_backends.push("CPU".into());

    // 7. Storage in GB
    let storage_gb = probes
        .storage_total_bytes
        .map(|bytes| (bytes as f64 / 1_000_000_000.0).round());

    HardwareProfile {
        os: "Windows".into(),
        os_version,
        architecture,
        chip,
        gpu,
        memory_gb,
        unified_memory,
        is_apple_silicon: false,
        compute_backends,
        detection_source: "native".into(),
        storage_gb,
        npu: probes.npu_name.clone(),
    }
}

#[cfg(target_os = "windows")]
mod win32 {
    use std::os::windows::ffi::OsStrExt;

    pub const HKEY_LOCAL_MACHINE: isize = -2147483646isize; // (0x80000002u32 as i32) as isize
    pub const KEY_READ: u32 = 0x20019;

    #[repr(C)]
    pub struct MemoryStatusEx {
        pub dw_length: u32,
        pub dw_memory_load: u32,
        pub ull_total_phys: u64,
        pub ull_avail_phys: u64,
        pub ull_total_page_file: u64,
        pub ull_avail_page_file: u64,
        pub ull_total_virtual: u64,
        pub ull_avail_virtual: u64,
        pub ull_avail_extended_virtual: u64,
    }

    #[link(name = "kernel32")]
    extern "system" {
        pub fn GlobalMemoryStatusEx(lp_buffer: *mut MemoryStatusEx) -> i32;
        pub fn GetDiskFreeSpaceExW(
            lp_directory_name: *const u16,
            lp_free_bytes_available_to_caller: *mut u64,
            lp_total_number_of_bytes: *mut u64,
            lp_total_number_of_free_bytes: *mut u64,
        ) -> i32;
    }

    #[link(name = "advapi32")]
    extern "system" {
        pub fn RegOpenKeyExW(
            h_key: isize,
            lp_sub_key: *const u16,
            ul_options: u32,
            sam_desired: u32,
            phk_result: *mut isize,
        ) -> i32;
        pub fn RegQueryValueExW(
            h_key: isize,
            lp_value_name: *const u16,
            lp_reserved: *mut u32,
            lp_type: *mut u32,
            lp_data: *mut u8,
            lpcb_data: *mut u32,
        ) -> i32;
        pub fn RegEnumKeyExW(
            h_key: isize,
            dw_index: u32,
            lp_name: *mut u16,
            lpcch_name: *mut u32,
            lp_reserved: *mut u32,
            lp_class: *mut u16,
            lpcch_class: *mut u32,
            lpft_last_write_time: *mut u64,
        ) -> i32;
        pub fn RegCloseKey(h_key: isize) -> i32;
    }

    pub fn to_wide(s: &str) -> Vec<u16> {
        std::ffi::OsStr::new(s)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    }
}

#[cfg(target_os = "windows")]
struct RegKey(isize);

#[cfg(target_os = "windows")]
impl RegKey {
    fn open(root: isize, subkey: &str) -> Option<Self> {
        let wide = win32::to_wide(subkey);
        let mut h_key: isize = 0;
        let res =
            unsafe { win32::RegOpenKeyExW(root, wide.as_ptr(), 0, win32::KEY_READ, &mut h_key) };
        if res == 0 {
            Some(RegKey(h_key))
        } else {
            None
        }
    }

    fn read_string(&self, value_name: &str) -> Option<String> {
        let wide_val = win32::to_wide(value_name);
        let mut val_type = 0u32;
        let mut byte_len = 0u32;
        let res = unsafe {
            win32::RegQueryValueExW(
                self.0,
                wide_val.as_ptr(),
                std::ptr::null_mut(),
                &mut val_type,
                std::ptr::null_mut(),
                &mut byte_len,
            )
        };
        if res != 0 || byte_len == 0 {
            return None;
        }

        let mut buf = vec![0u8; byte_len as usize];
        let res = unsafe {
            win32::RegQueryValueExW(
                self.0,
                wide_val.as_ptr(),
                std::ptr::null_mut(),
                &mut val_type,
                buf.as_mut_ptr(),
                &mut byte_len,
            )
        };
        if res != 0 {
            return None;
        }

        // 1: REG_SZ, 2: REG_EXPAND_SZ, 7: REG_MULTI_SZ
        if val_type == 1 || val_type == 2 || val_type == 7 {
            let u16_slice = unsafe {
                std::slice::from_raw_parts(buf.as_ptr() as *const u16, (byte_len / 2) as usize)
            };
            let s = String::from_utf16_lossy(u16_slice);
            let trimmed = s.trim_matches('\0').trim();
            if !trimmed.is_empty() {
                Some(trimmed.to_string())
            } else {
                None
            }
        } else {
            None
        }
    }

    fn read_u32(&self, value_name: &str) -> Option<u32> {
        let wide_val = win32::to_wide(value_name);
        let mut val_type = 0u32;
        let mut val = 0u32;
        let mut byte_len = std::mem::size_of::<u32>() as u32;
        let res = unsafe {
            win32::RegQueryValueExW(
                self.0,
                wide_val.as_ptr(),
                std::ptr::null_mut(),
                &mut val_type,
                &mut val as *mut u32 as *mut u8,
                &mut byte_len,
            )
        };
        if res == 0 && (val_type == 4 || val_type == 3) {
            Some(val)
        } else {
            None
        }
    }

    fn read_u64(&self, value_name: &str) -> Option<u64> {
        let wide_val = win32::to_wide(value_name);
        let mut val_type = 0u32;
        let mut val = 0u64;
        let mut byte_len = std::mem::size_of::<u64>() as u32;
        let res = unsafe {
            win32::RegQueryValueExW(
                self.0,
                wide_val.as_ptr(),
                std::ptr::null_mut(),
                &mut val_type,
                &mut val as *mut u64 as *mut u8,
                &mut byte_len,
            )
        };
        if res == 0 && (val_type == 11 || val_type == 3) {
            Some(val)
        } else {
            None
        }
    }

    fn enum_subkeys(&self) -> Vec<String> {
        let mut subkeys = Vec::new();
        let mut index = 0u32;
        let mut name_buf = [0u16; 256];
        loop {
            let mut name_len = name_buf.len() as u32;
            let res = unsafe {
                win32::RegEnumKeyExW(
                    self.0,
                    index,
                    name_buf.as_mut_ptr(),
                    &mut name_len,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                )
            };
            if res != 0 {
                break;
            }
            let name = String::from_utf16_lossy(&name_buf[..name_len as usize]);
            subkeys.push(name);
            index += 1;
        }
        subkeys
    }
}

#[cfg(target_os = "windows")]
impl Drop for RegKey {
    fn drop(&mut self) {
        if self.0 != 0 {
            unsafe { win32::RegCloseKey(self.0) };
        }
    }
}

#[cfg(target_os = "windows")]
pub fn native_windows_probes() -> WindowsProbes {
    let mut probes = WindowsProbes::default();

    // 1. Architecture: check host architecture (even if x86_64 binary runs under ARM64 emulation)
    let arch_env = std::env::var("PROCESSOR_ARCHITEW6432")
        .or_else(|_| std::env::var("PROCESSOR_ARCHITECTURE"))
        .unwrap_or_else(|_| std::env::consts::ARCH.to_string());
    probes.architecture = Some(arch_env);

    // 2. Registry: OS version
    if let Some(key) = RegKey::open(
        win32::HKEY_LOCAL_MACHINE,
        "SOFTWARE\\Microsoft\\Windows NT\\CurrentVersion",
    ) {
        probes.os_name = key.read_string("ProductName");
        probes.display_version = key.read_string("DisplayVersion");
        probes.build_number = key
            .read_string("CurrentBuildNumber")
            .or_else(|| key.read_string("CurrentBuild"));
        probes.ubr = key.read_u32("UBR");
    }

    // 3. Registry: CPU
    if let Some(key) = RegKey::open(
        win32::HKEY_LOCAL_MACHINE,
        "HARDWARE\\DESCRIPTION\\System\\CentralProcessor\\0",
    ) {
        probes.cpu_name = key
            .read_string("ProcessorNameString")
            .or_else(|| key.read_string("Identifier"));
    }

    // 4. Memory: GlobalMemoryStatusEx
    unsafe {
        let mut mem = win32::MemoryStatusEx {
            dw_length: std::mem::size_of::<win32::MemoryStatusEx>() as u32,
            dw_memory_load: 0,
            ull_total_phys: 0,
            ull_avail_phys: 0,
            ull_total_page_file: 0,
            ull_avail_page_file: 0,
            ull_total_virtual: 0,
            ull_avail_virtual: 0,
            ull_avail_extended_virtual: 0,
        };
        if win32::GlobalMemoryStatusEx(&mut mem) != 0 {
            probes.total_ram_bytes = Some(mem.ull_total_phys);
        }
    }

    // 5. Storage: GetDiskFreeSpaceExW
    unsafe {
        let root_wide = win32::to_wide("C:\\");
        let mut free_bytes_caller = 0u64;
        let mut total_bytes = 0u64;
        let mut total_free = 0u64;
        if win32::GetDiskFreeSpaceExW(
            root_wide.as_ptr(),
            &mut free_bytes_caller,
            &mut total_bytes,
            &mut total_free,
        ) != 0
        {
            probes.storage_total_bytes = Some(total_bytes);
            probes.storage_free_bytes = Some(free_bytes_caller);
        }
    }

    // 6. GPU Adapters from Display Adapter registry class
    let display_class_path =
        "SYSTEM\\CurrentControlSet\\Control\\Class\\{4d36e968-e325-11ce-bfc1-08002be10318}";
    if let Some(class_key) = RegKey::open(win32::HKEY_LOCAL_MACHINE, display_class_path) {
        for subkey_name in class_key.enum_subkeys() {
            if !subkey_name.chars().all(|c| c.is_ascii_digit()) {
                continue;
            }
            let sub_path = format!("{}\\{}", display_class_path, subkey_name);
            if let Some(adapter_key) = RegKey::open(win32::HKEY_LOCAL_MACHINE, &sub_path) {
                if let Some(name) = adapter_key.read_string("DriverDesc") {
                    let provider = adapter_key.read_string("ProviderName");
                    let driver_version = adapter_key.read_string("DriverVersion");
                    let dedicated_vram_bytes = adapter_key
                        .read_u64("HardwareInformation.qwMemorySize")
                        .or_else(|| {
                            adapter_key
                                .read_u32("HardwareInformation.MemorySize")
                                .map(|v| v as u64)
                        });
                    let has_vulkan_driver = adapter_key.read_string("VulkanDriverName").is_some();
                    if has_vulkan_driver {
                        probes.has_vulkan = true;
                    }
                    probes.gpus.push(WindowsGpuAdapter {
                        name,
                        provider,
                        dedicated_vram_bytes,
                        shared_ram_bytes: None,
                        driver_version,
                    });
                }
            }
        }
    }

    // 7. NPU detection
    // Microsoft Compute Accelerator Class:
    // HKLM\SYSTEM\CurrentControlSet\Control\Class\{f01a9d53-3ff6-48d2-9f97-c8a7004be10c}
    let npu_class_path =
        "SYSTEM\\CurrentControlSet\\Control\\Class\\{f01a9d53-3ff6-48d2-9f97-c8a7004be10c}";
    if let Some(class_key) = RegKey::open(win32::HKEY_LOCAL_MACHINE, npu_class_path) {
        for subkey_name in class_key.enum_subkeys() {
            if !subkey_name.chars().all(|c| c.is_ascii_digit()) {
                continue;
            }
            let sub_path = format!("{}\\{}", npu_class_path, subkey_name);
            if let Some(npu_key) = RegKey::open(win32::HKEY_LOCAL_MACHINE, &sub_path) {
                if let Some(name) = npu_key.read_string("DriverDesc") {
                    probes.npu_name = Some(name);
                    break;
                }
            }
        }
    }

    // Fallback NPU check: Qualcomm Hexagon NPU in ACPI enum
    if probes.npu_name.is_none() {
        let qcom_npu_path = "SYSTEM\\CurrentControlSet\\Enum\\ACPI\\QCOM0D0A";
        if let Some(qcom_key) = RegKey::open(win32::HKEY_LOCAL_MACHINE, qcom_npu_path) {
            for subkey_name in qcom_key.enum_subkeys() {
                let inst_path = format!("{}\\{}", qcom_npu_path, subkey_name);
                if let Some(inst_key) = RegKey::open(win32::HKEY_LOCAL_MACHINE, &inst_path) {
                    if let Some(desc) = inst_key.read_string("DeviceDesc") {
                        let name = desc.rsplit(';').next().unwrap_or(&desc).to_string();
                        probes.npu_name = Some(name);
                        break;
                    }
                }
            }
        }
    }

    // 8. Compute Backends detection
    let sys32 = std::path::Path::new("C:\\Windows\\System32");

    // DirectML: d3d12.dll and directml.dll
    probes.has_directml = sys32.join("d3d12.dll").exists() && sys32.join("directml.dll").exists();

    // Vulkan: vulkan-1.dll or previously detected driver
    if !probes.has_vulkan {
        probes.has_vulkan = sys32.join("vulkan-1.dll").exists();
    }

    // CUDA: NVIDIA GPU present + nvcuda.dll exists
    let has_nvidia_gpu = probes.gpus.iter().any(|g| {
        let name = g.name.to_lowercase();
        let prov = g.provider.as_deref().unwrap_or("").to_lowercase();
        name.contains("nvidia") || prov.contains("nvidia")
    });
    let has_cuda_dll = sys32.join("nvcuda.dll").exists();
    probes.has_cuda = has_nvidia_gpu && has_cuda_dll;

    probes
}

#[cfg(not(target_os = "windows"))]
pub fn native_windows_probes() -> WindowsProbes {
    WindowsProbes::default()
}

pub fn detect() -> HardwareProfile {
    profile_from_windows_probes(&native_windows_probes())
}
