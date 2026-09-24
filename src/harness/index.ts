import { invoke, isTauri } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import type { MaskPreview, RefinementSettings, AccessStatus, GenerationJob, StoredSource, BackendStatus, SetupProgress, SourceAsset, EngineProfile, GeneratedAsset, GenerationProgress, GenerationRequest, HardwareProfile, SculptHarness } from './types'

export type * from './types'

/** Design-preview fixture only. Native builds always query Rust for actual hardware. */
export const previewHardware: HardwareProfile = {
  os: 'macOS', osVersion: 'Preview fixture', architecture: 'arm64', chip: 'Apple M4',
  gpu: 'Apple M4', memoryGb: 16, unifiedMemory: true, isAppleSilicon: true,
  computeBackends: ['Metal', 'CPU'], detectionSource: 'preview', storageGb: 256,
}

export async function detectHardware(): Promise<HardwareProfile> {
  if (isTauri()) return invoke<HardwareProfile>('detect_hardware')
  return { ...previewHardware, computeBackends: [...previewHardware.computeBackends] }
}

function previewEngines(_profile: HardwareProfile): EngineProfile[] {
  return [
    {
      id: 'triposr', name: 'TripoSR', subtitle: 'Local reconstruction · Desktop required',
      description: 'Real geometry and vertex colors from a single image.',
      modelSizeGb: 1.8, runtime: 'PyTorch / Metal', compatibility: 'unsupported',
      reason: 'Open the Tauri desktop app for local inference. Browser previews cannot launch the Python worker.',
      implemented: true,
    },
    {
      id: 'demo', name: 'Workspace Demo', subtitle: 'Procedural sample · No AI',
      description: 'Explore the viewport. Does not reconstruct your image.',
      modelSizeGb: null, runtime: 'Local simulator', compatibility: 'recommended',
      reason: 'An explicit workspace demonstration using a procedural sculpture.',
      implemented: true,
    },
    {
      id: 'trellis-2', name: 'TRELLIS.2', subtitle: 'Future adapter',
      description: 'No validated adapter is included.', modelSizeGb: null,
      runtime: 'CUDA / PyTorch · planned', compatibility: 'unsupported',
      reason: 'This engine has not been integrated.', implemented: false,
    },
  ]
}

export async function backendStatus(): Promise<BackendStatus> {
  if (isTauri()) return invoke<BackendStatus>('backend_status')
  return { installed: false, engine: 'triposr', runtimePath: '', mpsAvailable: false,
    state: 'missing', downloadCacheBytes: 0, recommendedQuality: 'balanced', qualities: [],
    message: 'Open Sculpt desktop to use local AI reconstruction.' }
}

export async function installRuntime(onProgress: (progress: SetupProgress) => void, signal?: AbortSignal): Promise<BackendStatus> {
  if (!isTauri()) throw new Error('Runtime installation requires the Sculpt desktop app')
  if (signal?.aborted) throw abortError()
  const jobId = crypto.randomUUID()
  let started = false
  const cancel = () => { if (started) void invoke('cancel_generation', { jobId }).catch(() => {}) }
  const unlisten = await listen<SetupProgress>('sculpt://setup-progress', event => {
    if (event.payload.jobId !== jobId) return
    started = true
    if (signal?.aborted) cancel()
    onProgress(event.payload)
  })
  signal?.addEventListener('abort', cancel, { once: true })
  try {
    if (signal?.aborted) throw abortError()
    const status = await invoke<BackendStatus>('install_runtime', { jobId })
    if (signal?.aborted) throw abortError()
    return status
  } catch (error) {
    if (signal?.aborted) throw abortError()
    throw error instanceof Error ? error : new Error(String(error))
  } finally {
    signal?.removeEventListener('abort', cancel)
    unlisten()
  }
}

export async function importSource(name: string, dataUrl: string): Promise<SourceAsset | null> {
  if (!isTauri()) return null
  return invoke<SourceAsset>('import_source', { name, dataUrl })
}

export async function clearDownloadCache(): Promise<BackendStatus> {
  if (!isTauri()) throw new Error('Runtime storage requires the Sculpt desktop app')
  return invoke<BackendStatus>('clear_download_cache')
}

export async function readGeneratedAsset(assetId: string): Promise<ArrayBuffer> {
  if (!isTauri()) throw new Error('Real assets require the desktop app')
  // Tauri's binary IPC response avoids base64 and a second JSON-sized allocation.
  const bytes = await invoke<ArrayBuffer | number[]>('read_generated_asset', { assetId })
  return bytes instanceof ArrayBuffer ? bytes : Uint8Array.from(bytes).buffer
}

export async function saveGeneratedGlb(assetId: string, defaultName: string): Promise<string | null> {
  if (!isTauri()) throw new Error('Saved asset export requires the Sculpt desktop app')
  return invoke<string | null>('save_generated_glb', { assetId, defaultName })
}

const nativeAsset = (asset: GeneratedAsset): GeneratedAsset => ({ ...asset, generatedAt: new Date(Number(asset.generatedAt)).toISOString() })

export async function readSource(sourceId: string): Promise<StoredSource> {
  if (!isTauri()) throw new Error('Saved sources require the Sculpt desktop app')
  return invoke<StoredSource>('read_source', { sourceId })
}

export async function listGenerationJobs(): Promise<GenerationJob[]> {
  if (!isTauri()) return []
  const jobs = await invoke<GenerationJob[]>('list_generation_jobs', { limit: 50 })
  return jobs.map(job => ({ ...job, createdAt: new Date(Number(job.createdAt)).toISOString(),
    updatedAt: new Date(Number(job.updatedAt)).toISOString(), asset: job.asset ? nativeAsset(job.asset) : null }))
}

export async function getAccessStatus(): Promise<AccessStatus> {
  if (!isTauri()) return { mode: 'preview', canGenerate: false, freeGenerationsRemaining: null,
    activationAvailable: false, message: 'Open Sculpt desktop to reconstruct images. The browser offers a workspace demo.' }
  return invoke<AccessStatus>('get_access_status')
}

export async function activateLicense(signedLicense: string): Promise<AccessStatus> {
  if (!isTauri()) throw new Error('License activation requires the Sculpt desktop app')
  return invoke<AccessStatus>('activate_license', { signedLicense })
}

export async function getEngines(profile: HardwareProfile): Promise<EngineProfile[]> {
  if (isTauri()) return invoke<EngineProfile[]>('get_engines', { profile })
  return previewEngines(profile)
}

const abortError = () => new DOMException('Generation cancelled', 'AbortError')

function delay(ms: number, signal?: AbortSignal): Promise<void> {
  return new Promise((resolve, reject) => {
    if (signal?.aborted) { reject(abortError()); return }
    const abort = () => { clearTimeout(timer); reject(abortError()) }
    const timer = setTimeout(() => { signal?.removeEventListener('abort', abort); resolve() }, ms)
    signal?.addEventListener('abort', abort, { once: true })
  })
}

let previewBusy = false

async function generatePreview(request: GenerationRequest, onProgress: (value: GenerationProgress) => void, signal?: AbortSignal): Promise<GeneratedAsset> {
  if (signal?.aborted) throw abortError()
  if (!request.imageName.trim()) throw new Error('Select a source image first')
  const engine = previewEngines(previewHardware).find(profile => profile.id === request.engineId)
  if (!engine) throw new Error('Unknown engine profile')
  if (engine.compatibility === 'unsupported') throw new Error(engine.reason)
  if (previewBusy) throw new Error('A generation is already running')
  previewBusy = true
  const jobId = crypto.randomUUID()
  const stages = [
    { stage: 'analyzing', message: 'Analyzing image', start: 0, end: 18, ticks: 10 },
    { stage: 'geometry', message: 'Generating geometry', start: 18, end: 62, ticks: 22 },
    { stage: 'surface', message: 'Building surface', start: 62, end: 87, ticks: 14 },
    { stage: 'preparing', message: 'Preparing 3D asset', start: 87, end: 100, ticks: 9 },
  ] as const
  let progress = 0
  try {
    const tickMs = { draft: 95, balanced: 125, high: 155 }[request.geometry]
    for (const stage of stages) {
      for (let tick = 0; tick < stage.ticks; tick++) {
        if (signal?.aborted) throw abortError()
        progress = stage.start + (stage.end - stage.start) * tick / stage.ticks
        onProgress({ jobId, stage: stage.stage, progress, message: stage.message })
        await delay(tickMs, signal)
      }
    }
    if (signal?.aborted) throw abortError()
    let seed = 2166136261
    for (const byte of new TextEncoder().encode(request.imageName)) seed = Math.imul(seed ^ byte, 16777619) >>> 0
    onProgress({ jobId, stage: 'complete', progress: 100, message: 'Preview asset ready' })
    return { id: jobId, seed, generatedAt: new Date().toISOString(), simulated: true }
  } catch (error) {
    if (signal?.aborted) onProgress({ jobId, stage: 'cancelled', progress, message: 'Generation cancelled' })
    throw error
  } finally {
    previewBusy = false
  }
}

export async function generate(request: GenerationRequest, onProgress: (value: GenerationProgress) => void, signal?: AbortSignal): Promise<GeneratedAsset> {
  if (!isTauri()) return generatePreview(request, onProgress, signal)
  return nativeAsset(await runNativeOperation<GeneratedAsset>('generate_asset', { request }, onProgress, signal))
}

export async function refine(parentAssetId: string, settings: RefinementSettings, onProgress: (value: GenerationProgress) => void, signal?: AbortSignal): Promise<GeneratedAsset> {
  if (!isTauri()) throw new Error('Refinement requires the Sculpt desktop app')
  return nativeAsset(await runNativeOperation<GeneratedAsset>('refine_asset', { parentAssetId, settings }, onProgress, signal))
}

async function runNativeOperation<T>(command: string, args: Record<string, unknown>, onProgress: (value: GenerationProgress) => void, signal?: AbortSignal): Promise<T> {
  if (signal?.aborted) throw abortError()
  const jobId = crypto.randomUUID()
  let hasStarted = false
  let cancelRequested = false
  const cancel = () => {
    cancelRequested = true
    if (hasStarted) void invoke('cancel_generation', { jobId }).catch(() => { /* command result still reports failure */ })
  }
  // Listen before dispatch so the first native progress event cannot be missed.
  const unlisten = await listen<GenerationProgress>('sculpt://generation-progress', event => {
    if (event.payload.jobId !== jobId) return
    hasStarted = true
    if (cancelRequested) {
      void invoke('cancel_generation', { jobId }).catch(() => {})
    }
    onProgress(event.payload)
  })
  signal?.addEventListener('abort', cancel, { once: true })
  try {
    // Abort may have happened while the asynchronous event listener was registered.
    if (signal?.aborted) throw abortError()
    const asset = await invoke<T>(command, { ...args, jobId })
    if (signal?.aborted) throw abortError()
    return asset
  } catch (error) {
    if (signal?.aborted) throw abortError()
    throw error instanceof Error ? error : new Error(String(error))
  } finally {
    signal?.removeEventListener('abort', cancel)
    unlisten()
  }
}

export async function saveGlb(bytes: ArrayBuffer, defaultName: string): Promise<string | null> {
  if (isTauri()) return invoke<string | null>('save_glb', { bytes: Array.from(new Uint8Array(bytes)), defaultName })
  const data = new DataView(bytes)
  if (bytes.byteLength < 12 || data.getUint32(0, true) !== 0x46546c67 || data.getUint32(4, true) !== 2 || data.getUint32(8, true) !== bytes.byteLength) {
    throw new Error('Export is not a valid GLB file')
  }
  const url = URL.createObjectURL(new Blob([bytes], { type: 'model/gltf-binary' }))
  const anchor = document.createElement('a')
  anchor.href = url
  anchor.download = defaultName.endsWith('.glb') ? defaultName : `${defaultName}.glb`
  document.body.append(anchor)
  anchor.click()
  anchor.remove()
  setTimeout(() => URL.revokeObjectURL(url), 1000)
  return anchor.download
}

export async function prepareMask(sourceId: string, background: 'auto' | 'keep', onProgress: (value: GenerationProgress) => void, signal?: AbortSignal): Promise<MaskPreview> {
  if (!isTauri()) throw new Error('Foreground masking requires the Sculpt desktop app')
  return runNativeOperation<MaskPreview>('prepare_mask', { sourceId, background }, onProgress, signal)
}

export async function saveMask(sourceId: string, dataUrl: string): Promise<MaskPreview> {
  if (!isTauri()) throw new Error('Foreground masking requires the Sculpt desktop app')
  return invoke<MaskPreview>('save_mask', { sourceId, dataUrl })
}

export async function readMask(sourceId: string, sha256: string): Promise<MaskPreview> {
  if (!isTauri()) throw new Error('Foreground masking requires the Sculpt desktop app')
  return invoke<MaskPreview>('read_mask', { sourceId, sha256 })
}

export const sculptHarness: SculptHarness = { prepareMask, saveMask, readMask, detectHardware, getEngines, backendStatus, installRuntime, clearDownloadCache, importSource, readGeneratedAsset, readSource, listGenerationJobs, getAccessStatus, activateLicense, generate, refine, saveGeneratedGlb, saveGlb }
