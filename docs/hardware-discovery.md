# Sculpt Hardware Discovery Architecture

This document describes the platform-specific hardware discovery subsystem in Sculpt, which provides honest, truthful system capability information behind a common schema.

---

## 1. Principles

1. **Truthful inventory:** Hardware discovery inspects real hardware metrics (OS version, architecture, CPU brand, physical RAM, storage, GPUs, compute backends, and NPU).
2. **Unknown stays unknown:** If a metric or device cannot be detected or probed, it is reported as `Unknown`, `0.0`, or `None`. Specs and device identities are never fabricated or guessed.
3. **Hardware presence does not imply engine capability:** Detecting high-end hardware (such as an NVIDIA RTX GPU, Qualcomm Snapdragon X SoC, or multi-core CPU) records that the hardware exists and compute APIs are present, but never claims or asserts that a model can run without explicit target validation.
4. **Zero-overhead, private discovery:** All hardware probes run locally without network access, cloud telemetry, or third-party tracking.

---

## 2. Common Schema (`HardwareProfile`)

Hardware discovery outputs a canonical `HardwareProfile` serialized across Tauri IPC to the frontend:

```typescript
export interface HardwareProfile {
  os: string
  osVersion: string
  architecture: string
  chip: string
  gpu: string
  memoryGb: number
  unifiedMemory: boolean
  isAppleSilicon: boolean
  computeBackends: string[]
  detectionSource: 'native' | 'preview'
  storageGb?: number
  npu?: string
}
```

### Fields

- **`os`**: Canonical operating system name (`"macOS"`, `"Windows"`, `"Linux"`).
- **`osVersion`**: Detailed OS release and build string (e.g., `"15.2"` on macOS, `"11 24H2 (Build 26100.1742)"` on Windows, or `"Ubuntu 22.04.4 LTS"` on Linux).
- **`architecture`**: Hardware architecture normalized to `"arm64"` or `"x86_64"`.
- **`chip`**: Full CPU or SoC model name (e.g., `"Apple M4"`, `"Snapdragon(R) X Elite - X1E80100 - Qualcomm(R) Oryon(TM) CPU"`, `"AMD Ryzen 9 9950X 16-Core Processor"`).
- **`gpu`**: Composed display adapter name(s), joined by `" + "` for multi-adapter systems. Software-only fallback adapters are excluded when physical hardware is present.
- **`memoryGb`**: Total physical RAM in gigabytes, rounded to one decimal place.
- **`unifiedMemory`**: Boolean indicating whether system RAM is unified across CPU, GPU, and NPU (true for Apple Silicon and ARM64 SoCs such as Qualcomm Snapdragon X; false for systems with discrete GPU VRAM).
- **`isAppleSilicon`**: Boolean indicating Apple M-series hardware.
- **`computeBackends`**: Discovered compute API candidates (`"Metal"`, `"CUDA"`, `"DirectML"`, `"Vulkan"`, `"CPU"`). Ordered by preference; `"CPU"` is always present.
- **`detectionSource`**: `"native"` when probed on physical hardware, or `"preview"` for design mock fixtures.
- **`storageGb`**: Total capacity of the root/application drive in gigabytes.
- **`npu`**: Name of detected Neural Processing Unit (e.g., `"Apple Neural Engine"`, `"Snapdragon(R) X Plus - X1P42100 - Qualcomm(R) Hexagon(TM) NPU"`, `"Intel(R) AI Boost"`), or `None` if absent or not detectable.

---

## 3. Platform Providers

### 3.1 Windows Provider (x64 and ARM64)

- **Architecture:** Checks `PROCESSOR_ARCHITEW6432` and `PROCESSOR_ARCHITECTURE` to detect the native host architecture even when running under emulation, normalizing to `arm64` or `x86_64`.
- **OS Version:** Reads `HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion` (`ProductName`, `DisplayVersion`, `CurrentBuildNumber`, `UBR`).
- **CPU:** Reads `HKLM\HARDWARE\DESCRIPTION\System\CentralProcessor\0\ProcessorNameString`.
- **Memory:** Queries `GlobalMemoryStatusEx` for `ullTotalPhys`.
- **Storage:** Calls `GetDiskFreeSpaceExW` on the root drive (`C:\`).
- **Display Adapters:** Enumerates display adapters under `HKLM\SYSTEM\CurrentControlSet\Control\Class\{4d36e968-e325-11ce-bfc1-08002be10318}`. Dedicated video memory is extracted from `HardwareInformation.qwMemorySize` or `HardwareInformation.MemorySize`.
- **NPU:** Enumerates the Microsoft Compute Accelerator class (`{f01a9d53-3ff6-48d2-9f97-c8a7004be10c}`) and ACPI device enumeration (`QCOM0D0A`).
- **Compute Backends:**
  - `DirectML`: Detected if `d3d12.dll` and `directml.dll` are present.
  - `Vulkan`: Detected via `vulkan-1.dll` or registered display adapter Vulkan ICD.
  - `CUDA`: Detected when an NVIDIA display adapter is present with driver runtime (`nvcuda.dll`).
  - `CPU`: Always included.

### 3.2 Linux Provider (Ubuntu / Debian)

- **OS Version:** Parses `/etc/os-release` (`PRETTY_NAME`, `NAME`, `VERSION_ID`).
- **CPU:** Parses `/proc/cpuinfo` for `model name` or SoC `hardware`.
- **Memory:** Parses `/proc/meminfo` for `MemTotal`.
- **Storage:** Parses root filesystem capacity via `df -k /`.
- **Display Adapters & Compute Backends:**
  - `CUDA`: Probes `nvidia-smi` or `/proc/driver/nvidia/version`.
  - `Vulkan`: Probes `/usr/share/vulkan/icd.d` and `libvulkan.so.1`.
  - `CPU`: Always included.

### 3.3 macOS Provider (Apple Silicon and Intel)

- Preserves native macOS utilities:
  - `system_profiler SPHardwareDataType SPDisplaysDataType -detailLevel mini -json`
  - `sysctl machdep.cpu.brand_string`, `sysctl hw.optional.arm64`, `sysctl hw.memsize`
  - `sw_vers -productVersion`
  - `diskutil info /`
