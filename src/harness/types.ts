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
  /** Planning estimate, not a downloaded artifact size. */
  modelSizeGb: number | null
  runtime: string
  /** Suitability targets; inspect implemented before offering actual inference. */
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

export type RuntimeKind = 'mock' | 'pytorch' | 'mlx' | 'cuda-pytorch' | 'onnx' | 'webgpu' | 'native'
export type ModelState = 'not-installed' | 'downloading' | 'ready' | 'loading' | 'loaded' | 'error'

export type RuntimeAdapter = 'triposr' | 'demo'

export type ComputeDevice = 'mps' | 'cuda' | 'vulkan' | 'directml' | 'cpu'

export type InputModality = 'single_image' | 'multi_image' | 'text_prompt'

export type MaterialChannel =
  | 'vertex_color'
  | 'diffuse_color'
  | 'normal'
  | 'roughness'
  | 'metallic'
  | 'ambient_occlusion'
  | 'emissive'

export interface MaterialCapabilities {
  channels: MaterialChannel[]
  claimsPbr?: boolean
}

export interface ResourceThresholds {
  ramMinGb?: number | null
  ramRecommendedGb?: number | null
  vramMinGb?: number | null
  vramRecommendedGb?: number | null
  diskDownloadBytes?: number | null
  installedFootprintBytes?: number | null
}

export type OutputFormat = 'glb' | 'stl' | 'obj'

export interface EngineCapabilities {
  progressReporting: boolean
  cancellation: boolean
  refinement: boolean
  supportedOutputFormats: OutputFormat[]
}

export type TargetValidationStatus =
  | 'validated'
  | 'supported'
  | 'experimental'
  | 'compatible_unverified'
  | 'unsupported'

export interface EngineTarget {
  platform: 'macos' | 'windows' | 'linux' | 'any' | string
  architecture: 'arm64' | 'x86_64' | 'any' | string
  computeBackend: 'Metal' | 'CUDA' | 'Vulkan' | 'DirectML' | 'CPU' | string
  device: ComputeDevice
  minimumRamGb?: number | null
  recommendedRamGb?: number | null
  minimumVramGb?: number | null
  requiresAppleSilicon: boolean
  validated: boolean
  validationStatus?: TargetValidationStatus
}

export interface EngineManifest {
  schemaVersion: number
  id: string
  name: string
  description?: string | null
  publisher?: string | null
  upstreamUrl?: string | null
  licenseIdentifier: string
  engineRevision?: string | null
  runtimeSpecPath?: string | null
  adapter?: RuntimeAdapter | null
  implemented: boolean
  nativeHardwareRequired: boolean
  supportedInputs: InputModality[]
  materialCapabilities: MaterialCapabilities
  resourceThresholds?: ResourceThresholds | null
  capabilities: EngineCapabilities
  targets: EngineTarget[]
  knownLimits: string[]
  notes?: string | null
}

export interface EngineAvailability {
  engineId: string
  available: boolean
  device?: ComputeDevice | null
  reason: string
}

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

export interface MaskPreview {
  sourceId: string; sha256: string; width: number; height: number
  imageDataUrl: string; maskDataUrl: string
}

export interface GenerationRequest {
  engineId: string
  /** Native-owned validated image. Required for real inference. */
  sourceId?: string
  imageName: string
  geometry: 'draft' | 'balanced' | 'high'
  background?: 'auto' | 'keep'
  maskSha256?: string | null
  parentAssetId?: string | null
  refinement?: RefinementSettings | null
}

export interface RefinementSettings {
  resolution: number
  densityThreshold: number
  removeSmallComponents: boolean
  smoothingIterations: number
  textureResolution?: 'vertex' | '1k' | '2k'
  targetFaceCount?: number | null
}

export type GenerationStage = 'analyzing' | 'loading' | 'geometry' | 'surface' | 'preparing' | 'complete' | 'cancelled' | 'failed'

export interface GenerationProgress {
  jobId: string
  stage: GenerationStage
  progress: number
  message: string
}

export type AssetOperation = 'generate' | 'refine' | 'export' | 'import'

export type CoordinateSystem = 'glTF Y-up' | 'Z-up' | 'Y-up' | string

export type PhysicalUnit = 'meters' | 'millimeters' | 'centimeters' | 'inches' | 'unitless' | 'unknown' | string

export interface BoundingBox {
  min: [number, number, number]
  max: [number, number, number]
}

export type ManifestMaterialChannel =
  | 'vertex_color'
  | 'diffuse_texture'
  | 'diffuse_color'
  | 'normal'
  | 'roughness'
  | 'metallic'
  | 'ao'
  | 'ambient_occlusion'
  | 'emissive'
  | 'opacity'
  | string

export type TextureColorSpace = 'sRGB' | 'linear' | 'unknown'

export interface MaterialInventory {
  channels: ManifestMaterialChannel[]
  textureWidth?: number | null
  textureHeight?: number | null
  textureColorSpace?: TextureColorSpace | null
  pbr: boolean
  unknownChannels?: string[]
}

export interface OutputArtifact {
  path: string
  mimeType: string
  byteLength: number
  sha256: string
}

export interface ValidationWarning {
  code: string
  message: string
}

export interface SourceInputs {
  sourceImageId?: string | null
  sourceImageSha256?: string | null
  maskId?: string | null
  maskSha256?: string | null
  originalPrompt?: string | null
  composedPrompt?: string | null
}

export interface EngineProvenance {
  engineId: string
  engineRevision?: string | null
  modelWeightHashes?: Record<string, string>
  runtimeVersion?: string | null
  backend: string
  device: string
  driverVersion?: string | null
}

export interface GenerationParameters {
  quality?: string | null
  seed?: number | null
  resolution?: number | null
  densityThreshold?: number | null
  smoothingIterations?: number | null
  removeSmallComponents?: boolean | null
  targetFaceCount?: number | null
  extra?: Record<string, unknown>
}

export interface AssetLineage {
  parentAssetId?: string | null
  parentAssetVersion?: number | null
  operation: AssetOperation
  ancestorAssetIds?: string[]
}

export interface GeometryInventory {
  vertexCount: number
  faceCount: number
  componentCount?: number | null
  watertight?: boolean | null
  boundingBox: BoundingBox
  coordinateSystem: CoordinateSystem
  unit: PhysicalUnit
}

export interface AssetManifest {
  schemaVersion: number
  assetId: string
  createdAt: string
  sourceInputs: SourceInputs
  provenance: EngineProvenance
  parameters: GenerationParameters
  lineage: AssetLineage
  geometry: GeometryInventory
  materials: MaterialInventory
  artifacts: OutputArtifact[]
  warnings?: ValidationWarning[]
}

export interface GeneratedAsset {
  id: string
  seed: number
  generatedAt: string
  simulated: boolean
  metrics?: { device: string; totalSeconds: number; faces: number; vertices: number; sourceSha256?: string; canRefine?: boolean; operation?: 'generate' | 'refine'; resolution?: number; textureResolution?: string; refinement?: RefinementSettings; meshQuality?: { watertight: boolean; windingConsistent: boolean; components: number; degenerateFaces: number } }
  manifest?: AssetManifest | null
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
  prepareMask(sourceId: string, background: 'auto' | 'keep', onProgress: (value: GenerationProgress) => void, signal?: AbortSignal): Promise<MaskPreview>
  saveMask(sourceId: string, dataUrl: string): Promise<MaskPreview>
  readMask(sourceId: string, sha256: string): Promise<MaskPreview>
  detectHardware(): Promise<HardwareProfile>
  getEngines(profile: HardwareProfile): Promise<EngineProfile[]>
  backendStatus(): Promise<BackendStatus>
  installRuntime(onProgress: (progress: SetupProgress) => void, signal?: AbortSignal): Promise<BackendStatus>
  clearDownloadCache(): Promise<BackendStatus>
  importSource(name: string, dataUrl: string): Promise<SourceAsset | null>
  readGeneratedAsset(assetId: string): Promise<ArrayBuffer>
  readSource(sourceId: string): Promise<StoredSource>
  listGenerationJobs(): Promise<GenerationJob[]>
  generate(request: GenerationRequest, onProgress: (progress: GenerationProgress) => void, signal?: AbortSignal): Promise<GeneratedAsset>
  refine(parentAssetId: string, settings: RefinementSettings, onProgress: (progress: GenerationProgress) => void, signal?: AbortSignal): Promise<GeneratedAsset>
  saveGeneratedGlb(assetId: string, defaultName: string): Promise<string | null>
  saveGeneratedStl(assetId: string, defaultName: string, heightMm: number): Promise<string | null>
  saveGlb(bytes: ArrayBuffer, defaultName: string): Promise<string | null>
}
