# Sculpt Compatibility and Recommendation State Machine

This document specifies the algorithm, state machine, resource preflight checks, and CPU confirmation rules governing model availability, compatibility evaluation, and user-facing recommendations in Sculpt.

---

## 1. Architectural Principles

Sculpt evaluates local inference compatibility using strict separation of concerns across four distinct evaluation layers:

1. **Hardware Discovery (`HardwareProfile`):**
   Truthful probe of the host operating system, CPU architecture, physical RAM, free disk space, detected GPUs, and available compute backends (`Metal`, `CUDA`, `Vulkan`, `DirectML`, `CPU`).

2. **Declared Capability vs. Validated Support (`EngineTarget` & `EngineManifest`):**
   Data-driven engine descriptors declare candidate targets, resource thresholds, and explicit validation statuses. **Hardware presence does not imply engine capability**: the presence of high-end hardware (e.g., NVIDIA CUDA on Windows/Linux, Qualcomm Snapdragon on Windows ARM64, or multi-core x86_64 CPUs) does not grant model availability or inference capability unless Sculpt explicitly validates the specific target (`is_target_validated()`).

3. **Fine-Grained Compatibility Status (`CompatibilityStatus`):**
   Detailed technical compatibility reflecting whether the engine is tested, experimental, unverified, constrained by resources, or unsupported.

4. **Recommendation State (`recommended`, `available`, `unsupported`):**
   User-facing selector classification guiding the artist toward the most reliable starting point while keeping alternatives transparent.

---

## 2. State Machine Vocabulary

### 2.1 Fine-Grained Compatibility States (`CompatibilityStatus`)

| State | Definition | UI / Selector Action |
|---|---|---|
| `supported` | Tested and validated end-to-end on this exact platform, architecture, and compute backend. | Available for selection; recommended if baseline. |
| `compatible_unverified` | Declared candidate requirements appear satisfied, but end-to-end validation on this target is not yet complete. | Available with unverified disclosure. |
| `experimental` | Opt-in execution with known limitations, active development, or community runtime bindings. | Available with experimental disclosure. |
| `insufficient_resources` | Detected system RAM, VRAM, or disk storage is below the engine's mandatory admission floor. | Disabled with explicit resource shortfall notice. |
| `runtime_missing` | Target is supported, but local Python virtual environment, dependencies, or model weights require installation or repair. | Available with setup/repair prompt. |
| `unsupported` | No compatible target exists for this OS/architecture, compute backend is absent, or engine adapter is unimplemented. | Disabled with clear explanation. |

### 2.2 Recommendation Tiers (`compatibility`)

- **`recommended`:** The primary suggested engine for the detected device. On validated Apple Silicon hardware with $\ge 16\text{ GB}$ RAM, TripoSR is recommended. When no AI engine is validated for the current machine, the zero-dependency Workspace Demo is elevated to `recommended`.
- **`available`:** The engine can execute on this machine (as a secondary supported adapter, an unverified candidate, or an experimental target), but is not the primary recommendation.
- **`unsupported`:** The engine cannot run due to missing backends, insufficient resources, or missing implementation.

---

## 3. Resource Preflight Checks (`ResourcePreflight`)

Resource preflight checks evaluate physical hardware constraints deterministically before any model download, worker launch, or generation is permitted.

### 3.1 Preflight Algorithm

```text
Input: HardwareProfile, EngineManifest, Selected EngineTarget
Output: ResourcePreflight { passed, ram_passed, storage_passed, details }

1. Check RAM Floor:
   ram_minimum = target.minimum_ram_gb ?? manifest.resource_thresholds.ram_min_gb
   if ram_minimum exists:
     if detected_ram is not finite or detected_ram < ram_minimum:
       ram_passed = false
       add detail: "Detected RAM (<detected> GB) is below required minimum (<min> GB)."
     else:
       ram_passed = true
       if detected_ram < ram_recommended:
         add detail: "Detected RAM meets minimum (<min> GB) but is below recommended (<rec> GB)."
       else:
         add detail: "Detected RAM meets recommended specification (<rec> GB)."
   else:
     ram_passed = true

2. Check Storage Footprint:
   required_storage = manifest.resource_thresholds.installed_footprint_bytes ?? disk_download_bytes
   if required_storage exists and detected_storage exists:
     if detected_storage < required_storage:
       storage_passed = false
       add detail: "Detected storage (<detected> GB) is below estimated footprint (<req> GB)."
     else:
       storage_passed = true
       add detail: "Detected storage is sufficient for estimated footprint."
   else:
     storage_passed = true

3. Overall Result:
   passed = ram_passed && storage_passed
   if !ram_passed or !storage_passed:
     compatibility_status = InsufficientResources
     compatibility = unsupported
```

### 3.2 Key Resource Boundaries

- **Admission Floor vs. Peak Working Set:** Minimum RAM represents the physical admission floor below which model weights cannot safely load without OS thrashing or out-of-memory crashes.
- **Unified Memory:** On Apple Silicon architectures with unified memory (`unified_memory: true`), system RAM is shared dynamically between CPU and GPU/Metal cores.
- **Preflight Gating:** Preflight failures transition the engine immediately to `insufficient_resources` and `unsupported`, preventing allocation of worker processes or download caches.

---

## 4. CPU Fallback Policy and Confirmation Rules

### 4.1 Strict No-Silent-Downgrade Guarantee

Sculpt enforces three absolute rules regarding CPU inference:

1. **NEVER silently downgrade:** If a GPU accelerator (Metal, CUDA, Vulkan, DirectML) fails, is missing, or runs out of memory, Sculpt **NEVER** silently falls back to CPU execution.
2. **NEVER silently substitute:** Sculpt **NEVER** substitutes another engine or model without explicit user selection.
3. **Explicit Engine Support Required:** An engine may only execute on CPU if its manifest explicitly provides a non-unsupported CPU target. For example, TripoSR's CPU target is explicitly marked `validation_status: "unsupported"`, so TripoSR cannot run on CPU under any circumstances.

### 4.2 Explicit Confirmation Requirement

When an AI reconstruction engine supports CPU execution and CPU is selected:

- `requires_confirmation`: set to `true`.
- `requires_cpu_confirmation`: set to `true`.
- `confirmation_reason`: set to an explicit warning:
  > *"CPU execution is significantly slower than GPU acceleration and may cause high CPU and thermal load. Explicit confirmation is required to run on CPU."*
- **Resource Gating:** CPU execution is subject to the same minimum RAM preflight checks. If physical RAM is insufficient, CPU execution is rejected with `insufficient_resources`.

### 4.3 Procedural Samples Exception

The zero-dependency Workspace Demo generates a procedural sculpture without machine learning inference or neural network evaluation. Because it executes lightweight procedural geometry code, it runs on CPU with `requires_confirmation: false` and `requires_cpu_confirmation: false`.

---

## 5. Contract Specification

### 5.1 Rust Data Structures

```rust
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CompatibilityStatus {
    Supported,
    CompatibleUnverified,
    Experimental,
    InsufficientResources,
    RuntimeMissing,
    Unsupported,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ResourcePreflight {
    pub passed: bool,
    pub ram_passed: bool,
    pub storage_passed: bool,
    pub ram_detected_gb: f64,
    pub ram_minimum_gb: Option<f64>,
    pub ram_recommended_gb: Option<f64>,
    pub storage_detected_gb: Option<f64>,
    pub storage_required_gb: Option<f64>,
    pub details: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct EngineProfile {
    pub id: String,
    pub name: String,
    pub subtitle: String,
    pub description: String,
    pub model_size_gb: Option<f64>,
    pub runtime: String,
    pub compatibility: String,
    pub compatibility_status: CompatibilityStatus,
    pub reason: String,
    pub recommendation_reason: Option<String>,
    pub requires_confirmation: bool,
    pub confirmation_reason: Option<String>,
    #[serde(default)]
    pub requires_cpu_confirmation: bool,
    pub resource_preflight: ResourcePreflight,
    pub implemented: bool,
}
```

### 5.2 TypeScript Data Structures

```typescript
export type CompatibilityStatus =
  | 'supported'
  | 'compatible_unverified'
  | 'experimental'
  | 'insufficient_resources'
  | 'runtime_missing'
  | 'unsupported'

export interface ResourcePreflight {
  passed: boolean
  ramPassed: boolean
  storagePassed: boolean
  ramDetectedGb: number
  ramMinimumGb?: number | null
  ramRecommendedGb?: number | null
  storageDetectedGb?: number | null
  storageRequiredGb?: number | null
  details: string[]
}

export interface EngineProfile {
  id: string
  name: string
  subtitle: string
  description: string
  modelSizeGb: number | null
  runtime: string
  compatibility: 'recommended' | 'available' | 'unsupported'
  compatibilityStatus: CompatibilityStatus
  reason: string
  recommendationReason?: string | null
  requiresConfirmation: boolean
  confirmationReason?: string | null
  requiresCpuConfirmation?: boolean
  resourcePreflight: ResourcePreflight
  implemented: boolean
}
```
