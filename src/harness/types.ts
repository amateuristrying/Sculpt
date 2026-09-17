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

export type RuntimeKind = 'mock' | 'pytorch' | 'mlx' | 'cuda-pytorch' | 'onnx' | 'webgpu' | 'native'
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
  /** Native-owned validated image. Required for real inference. */
  sourceId?: string
  imageName: string
  geometry: 'draft' | 'balanced' | 'high'
  background?: 'auto' | 'keep'
  parentAssetId?: string | null
  refinement?: RefinementSettings | null
}

export interface RefinementSettings {
  resolution: number
  densityThreshold: number
  removeSmallComponents: boolean
  smoothingIterations: number
}

export type GenerationStage = 'analyzing' | 'loading' | 'geometry' | 'surface' | 'preparing' | 'complete' | 'cancelled' | 'failed'

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
  simulated: boolean
  metrics?: { device: string; totalSeconds: number; faces: number; vertices: number; sourceSha256?: string; canRefine?: boolean; operation?: 'generate' | 'refine'; resolution?: number; refinement?: RefinementSettings; meshQuality?: { watertight: boolean; windingConsistent: boolean; components: number; degenerateFaces: number } }
}

export interface SourceAsset {
  id: string
  name: string
  mimeType: string
  width: number
  height: number
  sha256: string
}

export interface StoredSource { asset: SourceAsset; dataUrl: string; size: number }

export interface GenerationJob {
  id: string
  request: GenerationRequest
  state: 'running' | 'succeeded' | 'failed' | 'cancelled' | 'interrupted'
  createdAt: string
  updatedAt: string
  error?: string | null
  asset?: GeneratedAsset | null
}

export interface AccessStatus {
  mode: 'development' | 'trial' | 'paid' | 'preview'
  canGenerate: boolean
  freeGenerationsRemaining: number | null
  activationAvailable: boolean
  message: string
}

export interface BackendStatus {
  installed: boolean
  engine: string
  runtimePath: string
  message: string
  mpsAvailable: boolean
  state: 'missing' | 'repair' | 'ready'
  downloadCacheBytes: number
  recommendedQuality: 'draft' | 'balanced' | 'high'
  qualities: { id: 'draft' | 'balanced' | 'high'; resolution: number; estimatedMemoryGb: number; recommendedRamGb: number }[]
}

export interface SetupProgress { jobId: string; stage: string; progress: number; message: string }

export interface SculptHarness {
  detectHardware(): Promise<HardwareProfile>
  getEngines(profile: HardwareProfile): Promise<EngineProfile[]>
  backendStatus(): Promise<BackendStatus>
  installRuntime(onProgress: (progress: SetupProgress) => void, signal?: AbortSignal): Promise<BackendStatus>
  clearDownloadCache(): Promise<BackendStatus>
  importSource(name: string, dataUrl: string): Promise<SourceAsset | null>
  readGeneratedAsset(assetId: string): Promise<ArrayBuffer>
  readSource(sourceId: string): Promise<StoredSource>
  listGenerationJobs(): Promise<GenerationJob[]>
  getAccessStatus(): Promise<AccessStatus>
  activateLicense(signedLicense: string): Promise<AccessStatus>
  generate(request: GenerationRequest, onProgress: (progress: GenerationProgress) => void, signal?: AbortSignal): Promise<GeneratedAsset>
  refine(parentAssetId: string, settings: RefinementSettings, onProgress: (progress: GenerationProgress) => void, signal?: AbortSignal): Promise<GeneratedAsset>
  saveGeneratedGlb(assetId: string, defaultName: string): Promise<string | null>
  saveGlb(bytes: ArrayBuffer, defaultName: string): Promise<string | null>
}
