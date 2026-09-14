import { invoke, isTauri } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import type { BackendStatus, SourceAsset, EngineProfile, GeneratedAsset, GenerationProgress, GenerationRequest, HardwareProfile, SculptHarness } from './types'

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
    message: 'Open Sculpt desktop to use local AI reconstruction.' }
}

export async function importSource(name: string, dataUrl: string): Promise<SourceAsset | null> {
  if (!isTauri()) return null
  return invoke<SourceAsset>('import_source', { name, dataUrl })
}

export async function readGeneratedAsset(assetId: string): Promise<ArrayBuffer> {
  if (!isTauri()) throw new Error('Real assets require the desktop app')
  const base64 = await invoke<string>('read_generated_asset', { assetId })
  return Uint8Array.from(atob(base64), char => char.charCodeAt(0)).buffer
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
    const asset = await invoke<GeneratedAsset>('generate_asset', { request, jobId })
    if (signal?.aborted) throw abortError()
    return { ...asset, generatedAt: new Date(Number(asset.generatedAt)).toISOString() }
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

export const sculptHarness: SculptHarness = { detectHardware, getEngines, backendStatus, importSource, readGeneratedAsset, generate, saveGlb }
