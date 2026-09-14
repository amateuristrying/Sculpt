/** Public Sculpt Inference Harness contract. UI code depends on this, never a model SDK. */
export interface HardwareProfile {
  os: string
  osVersion: string
  architecture: string
  chip: string
  gpu: string
  memoryGb: number
  unifiedMemory: boolean
  isAppleSilicon: boolean
  /** Hardware APIs detected; this does not imply an ML runtime is installed. */
  computeBackends: string[]
  detectionSource: 'native' | 'preview'
  storageGb?: number
}

export interface EngineProfile {
  id: string
  name: string
  subtitle: string
  description: string
  /** Planning estimate, not a downloaded artifact size. */
  modelSizeGb: number | null
  runtime: string
  /** Suitability targets; inspect implemented before offering actual inference. */
  compatibility: 'recommended' | 'available' | 'unsupported'
  reason: string
  implemented: boolean
}

export type RuntimeKind = 'mock' | 'mlx' | 'cuda-pytorch' | 'onnx' | 'webgpu' | 'native'
export type ModelState = 'not-installed' | 'downloading' | 'ready' | 'loading' | 'loaded' | 'error'

/** Future adapters may launch a Python worker or native process; Rust owns orchestration. */
export interface RuntimeDescriptor {
  id: RuntimeKind
  name: string
  installed: boolean
  available: boolean
  computeBackend: string
}

export interface ModelArtifact {
  engineId: string
  version: string
  state: ModelState
  downloadBytes?: number
  minimumMemoryGb: number
  compatibleRuntimes: RuntimeKind[]
}

export interface GenerationRequest {
  engineId: string
  /** Prototype fixture identity only. No image pixels are passed to an AI engine yet.
   * A real adapter should receive a native-owned source asset ID after image import,
   * letting Rust validate/decode the local file without exposing paths to model UI code.
   */
  imageName: string
  geometry: 'draft' | 'balanced' | 'high'
}

export type GenerationStage = 'analyzing' | 'geometry' | 'surface' | 'preparing' | 'complete' | 'cancelled' | 'failed'

export interface GenerationProgress {
  jobId: string
  stage: GenerationStage
  progress: number
  message: string
}

export interface GeneratedAsset {
  id: string
  seed: number
  generatedAt: string
  simulated: true
}

export interface SculptHarness {
  detectHardware(): Promise<HardwareProfile>
  getEngines(profile: HardwareProfile): Promise<EngineProfile[]>
  generate(request: GenerationRequest, onProgress: (progress: GenerationProgress) => void, signal?: AbortSignal): Promise<GeneratedAsset>
  saveGlb(bytes: ArrayBuffer, defaultName: string): Promise<string | null>
}
