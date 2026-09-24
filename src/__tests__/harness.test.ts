import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'
import { invoke, isTauri } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { backendStatus, installRuntime, clearDownloadCache, importSource, readGeneratedAsset, detectHardware, generate, getEngines, previewHardware, saveGlb, saveGeneratedGlb, refine, readSource, listGenerationJobs, getAccessStatus, activateLicense } from '../harness'
import type { GenerationProgress, GenerationRequest, HardwareProfile } from '../harness'

vi.mock('@tauri-apps/api/core', () => ({ isTauri: vi.fn(() => false), invoke: vi.fn() }))
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn() }))

const request: GenerationRequest = { engineId: 'demo', imageName: 'study.png', geometry: 'draft' }

beforeEach(() => {
  vi.resetAllMocks()
  vi.mocked(isTauri).mockReturnValue(false)
})

afterEach(() => {
  vi.useRealTimers()
})

describe('hardware-aware engine catalog', () => {
  it('labels browser hardware as a preview fixture and returns independent copies', async () => {
    const hardware = await detectHardware()
    expect(hardware).toMatchObject({ chip: 'Apple M4', memoryGb: 16, storageGb: 256, detectionSource: 'preview' })
    hardware.computeBackends.length = 0
    expect((await detectHardware()).computeBackends).toContain('Metal')
  })

  it('offers only explicit demo generation in a browser', async () => {
    const catalog = await getEngines(previewHardware)
    expect(catalog.find(engine => engine.id === 'triposr')?.compatibility).toBe('unsupported')
    expect(catalog.find(engine => engine.id === 'trellis-2')?.compatibility).toBe('unsupported')
    expect(catalog.find(engine => engine.id === 'demo')?.implemented).toBe(true)
    expect(catalog.find(engine => engine.id === 'triposr')?.reason).toMatch(/desktop app/)
  })

  it.each<[string, Partial<HardwareProfile>]>([
    ['Intel Mac with Metal', { chip: 'Intel Core i7', architecture: 'x86_64', isAppleSilicon: false, unifiedMemory: false }],
    ['unknown hardware', { chip: 'Unknown processor', gpu: 'GPU not detected', memoryGb: 0, architecture: 'unknown', computeBackends: ['CPU'], isAppleSilicon: false }],
    ['Apple Silicon without detected Metal', { computeBackends: ['CPU'] }],
    ['Apple Silicon below the memory target', { memoryGb: 8 }],
  ])('does not offer the Apple target on %s', async (_name, changes) => {
    const catalog = await getEngines({ ...previewHardware, ...changes })
    expect(catalog.find(engine => engine.id === 'triposr')?.compatibility).toBe('unsupported')
    expect(catalog.find(engine => engine.id === 'demo')?.compatibility).toBe('recommended')
    expect(catalog.find(engine => engine.id === 'demo')?.implemented).toBe(true)
  })
})

describe('mock generation job lifecycle', () => {
  it('advances through ordered stages and returns a clearly simulated asset', async () => {
    vi.useFakeTimers()
    const events: GenerationProgress[] = []
    const result = generate(request, event => events.push(event))
    await vi.runAllTimersAsync()
    const asset = await result
    expect([...new Set(events.map(event => event.stage))]).toEqual(['analyzing', 'geometry', 'surface', 'preparing', 'complete'])
    expect(events[0].progress).toBe(0)
    expect(events.at(-1)?.progress).toBe(100)
    expect(events.every((event, i) => event.progress >= (events[i - 1]?.progress ?? 0) && event.progress <= 100)).toBe(true)
    expect(events.every(event => event.jobId === asset.id)).toBe(true)
    expect(asset.simulated).toBe(true)
    expect(Number.isFinite(Date.parse(asset.generatedAt))).toBe(true)
    expect(Number.isInteger(asset.seed)).toBe(true)
  })

  it('cancels mid-generation, stops progress, and releases the single-job slot', async () => {
    vi.useFakeTimers()
    const controller = new AbortController()
    const events: GenerationProgress[] = []
    const running = generate(request, event => events.push(event), controller.signal)
    const rejected = expect(running).rejects.toMatchObject({ name: 'AbortError' })
    await vi.advanceTimersByTimeAsync(1500)
    expect(events.some(event => event.stage === 'geometry')).toBe(true)
    controller.abort()
    await rejected
    expect(events.at(-1)?.stage).toBe('cancelled')
    const eventCount = events.length
    await vi.runAllTimersAsync()
    expect(events).toHaveLength(eventCount)
    expect(events.some(event => event.stage === 'complete')).toBe(false)

    const next = generate(request, () => {})
    await vi.runAllTimersAsync()
    expect((await next).simulated).toBe(true)
  })

  it('rejects overlapping jobs, and an already cancelled request never starts', async () => {
    vi.useFakeTimers()
    const preCancelled = new AbortController()
    preCancelled.abort()
    const onProgress = vi.fn()
    await expect(generate(request, onProgress, preCancelled.signal)).rejects.toMatchObject({ name: 'AbortError' })
    expect(onProgress).not.toHaveBeenCalled()

    const first = generate(request, () => {})
    await expect(generate(request, () => {})).rejects.toThrow(/already running/)
    await vi.runAllTimersAsync()
    await first
  })

  it.each([
    [{ ...request, imageName: '  ' }, /source image/],
    [{ ...request, engineId: 'unknown-engine' }, /Unknown engine/],
    [{ ...request, engineId: 'trellis-2' }, /not been integrated/],
  ] as const)('rejects unavailable input before emitting progress', async (invalid, message) => {
    const onProgress = vi.fn()
    await expect(generate(invalid, onProgress)).rejects.toThrow(message)
    expect(onProgress).not.toHaveBeenCalled()
  })
})

describe('GLB save boundary', () => {
  it('rejects empty, corrupt, and truncated data before attempting a browser download', async () => {
    await expect(saveGlb(new ArrayBuffer(0), 'study.glb')).rejects.toThrow(/valid GLB/)
    await expect(saveGlb(new ArrayBuffer(16), 'study.glb')).rejects.toThrow(/valid GLB/)
    const truncated = new ArrayBuffer(12)
    const header = new DataView(truncated)
    header.setUint32(0, 0x46546c67, true)
    header.setUint32(4, 2, true)
    header.setUint32(8, 100, true)
    await expect(saveGlb(truncated, 'study.glb')).rejects.toThrow(/valid GLB/)
  })
})

describe('native harness bridge', () => {
  it('never offers installation or cache deletion in the browser', async () => {
    await expect(installRuntime(() => {})).rejects.toThrow('desktop')
    await expect(clearDownloadCache()).rejects.toThrow('desktop')
    expect(invoke).not.toHaveBeenCalled()
  })

  it('cancels installation during listener registration without starting native work', async () => {
    vi.mocked(isTauri).mockReturnValue(true)
    const controller = new AbortController(), unlisten = vi.fn()
    let register!: (callback: () => void) => void
    vi.mocked(listen).mockReturnValueOnce(new Promise(resolve => { register = resolve }))
    const running = installRuntime(() => {}, controller.signal)
    const rejected = expect(running).rejects.toMatchObject({ name: 'AbortError' })
    controller.abort(); register(unlisten)
    await rejected
    expect(invoke).not.toHaveBeenCalled()
    expect(unlisten).toHaveBeenCalledOnce()
  })

  it('cleans up the setup listener after a native download failure', async () => {
    vi.mocked(isTauri).mockReturnValue(true)
    const unlisten = vi.fn()
    vi.mocked(listen).mockResolvedValueOnce(unlisten)
    vi.mocked(invoke).mockRejectedValueOnce('Download interrupted')
    await expect(installRuntime(() => {})).rejects.toThrow('Download interrupted')
    expect(unlisten).toHaveBeenCalledOnce()
  })

  it('returns the verified native installation status', async () => {
    vi.mocked(isTauri).mockReturnValue(true)
    const unlisten = vi.fn()
    vi.mocked(listen).mockResolvedValueOnce(unlisten)
    vi.mocked(invoke).mockResolvedValueOnce({ installed: true, state: 'ready' })
    expect(await installRuntime(() => {})).toMatchObject({ installed: true, state: 'ready' })
    expect(invoke).toHaveBeenCalledWith('install_runtime', { jobId: expect.any(String) })
    expect(unlisten).toHaveBeenCalledOnce()
  })

  it('passes actual image bytes to Rust and receives an opaque source ID', async () => {
    vi.mocked(isTauri).mockReturnValue(true)
    vi.mocked(invoke).mockResolvedValueOnce({ id: 'source-1', sha256: 'abc' })
    expect(await importSource('banana.png', 'data:image/png;base64,cGl4ZWxz')).toMatchObject({ id: 'source-1' })
    expect(invoke).toHaveBeenCalledWith('import_source', { name: 'banana.png', dataUrl: 'data:image/png;base64,cGl4ZWxz' })
  })
  it('decodes native asset bytes and never manufactures a mock result on failure', async () => {
    vi.mocked(isTauri).mockReturnValue(true)
    const binary = new TextEncoder().encode('glTF').buffer
    vi.mocked(invoke).mockResolvedValueOnce(binary)
    expect(new TextDecoder().decode(await readGeneratedAsset('asset-id'))).toBe('glTF')
    expect(invoke).toHaveBeenCalledWith('read_generated_asset', { assetId: 'asset-id' })
    vi.mocked(invoke).mockResolvedValueOnce([103, 108, 84, 70])
    expect(new TextDecoder().decode(await readGeneratedAsset('numeric-fallback'))).toBe('glTF')
    vi.mocked(invoke).mockRejectedValueOnce(new Error('Asset unavailable'))
    await expect(readGeneratedAsset('missing')).rejects.toThrow('Asset unavailable')
  })
  it('reports browser inference unavailable without invoking native commands', async () => {
    expect((await backendStatus()).installed).toBe(false)
    expect(await importSource('image.png', 'data:')).toBe(null)
    await expect(readGeneratedAsset('id')).rejects.toThrow('desktop')
    expect(invoke).not.toHaveBeenCalled()
  })

  it('uses detected native hardware rather than the browser fixture', async () => {
    vi.mocked(isTauri).mockReturnValue(true)
    const nativeHardware: HardwareProfile = { ...previewHardware, chip: 'Intel Core i7', isAppleSilicon: false, detectionSource: 'native' }
    vi.mocked(invoke).mockResolvedValueOnce(nativeHardware)
    expect(await detectHardware()).toEqual(nativeHardware)
    expect(invoke).toHaveBeenCalledWith('detect_hardware')
  })

  it('normalizes native timestamps and always releases its event listener', async () => {
    vi.mocked(isTauri).mockReturnValue(true)
    const unlisten = vi.fn()
    vi.mocked(listen).mockResolvedValueOnce(unlisten)
    vi.mocked(invoke).mockResolvedValueOnce({ id: 'native-job', seed: 42, simulated: true, generatedAt: '1700000000000' })
    const result = await generate(request, () => {})
    expect(result.generatedAt).toBe('2023-11-14T22:13:20.000Z')
    expect(invoke).toHaveBeenCalledWith('generate_asset', expect.objectContaining({ request, jobId: expect.any(String) }))
    expect(unlisten).toHaveBeenCalledOnce()
  })

  it('honors cancellation during listener setup without dispatching a native job', async () => {
    vi.mocked(isTauri).mockReturnValue(true)
    const unlisten = vi.fn()
    let register!: (callback: () => void) => void
    vi.mocked(listen).mockReturnValueOnce(new Promise(resolve => { register = resolve }))
    const controller = new AbortController()
    const running = generate(request, () => {}, controller.signal)
    const rejected = expect(running).rejects.toMatchObject({ name: 'AbortError' })
    controller.abort()
    register(unlisten)
    await rejected
    expect(invoke).not.toHaveBeenCalled()
    expect(unlisten).toHaveBeenCalledOnce()
  })
})

describe('saved assets and refinement boundary', () => {
  const settings = { resolution: 192, densityThreshold: 22, removeSmallComponents: true, smoothingIterations: 3 }

  it('normalizes saved job and asset timestamps while preserving requests and failed records', async () => {
    vi.mocked(isTauri).mockReturnValue(true)
    const savedRequest = { ...request, engineId: 'triposr', sourceId: 'source-1' }
    vi.mocked(invoke).mockResolvedValueOnce([
      { id: 'saved-1', request: savedRequest, state: 'succeeded', createdAt: '1700000000000', updatedAt: '1700000001000', asset: { id: 'saved-1', seed: 0, simulated: false, generatedAt: '1700000001000' } },
      { id: 'failed-2', request: savedRequest, state: 'failed', createdAt: '1700000002000', updatedAt: '1700000003000', error: 'No foreground found', asset: null },
    ])
    const jobs = await listGenerationJobs()
    expect(invoke).toHaveBeenCalledWith('list_generation_jobs', { limit: 50 })
    expect(jobs[0]).toMatchObject({ request: savedRequest, createdAt: '2023-11-14T22:13:20.000Z', updatedAt: '2023-11-14T22:13:21.000Z', asset: { generatedAt: '2023-11-14T22:13:21.000Z' } })
    expect(jobs[1]).toMatchObject({ state: 'failed', asset: null, error: 'No foreground found' })
  })

  it('exports a saved asset by native ID without uploading GLB bytes through JSON', async () => {
    vi.mocked(isTauri).mockReturnValue(true)
    vi.mocked(invoke).mockResolvedValueOnce('/exports/banana.glb').mockResolvedValueOnce(null)
    expect(await saveGeneratedGlb('asset-1', 'banana.glb')).toBe('/exports/banana.glb')
    expect(invoke).toHaveBeenCalledWith('save_generated_glb', { assetId: 'asset-1', defaultName: 'banana.glb' })
    expect(await saveGeneratedGlb('asset-1', 'banana.glb')).toBeNull()
    expect(invoke).not.toHaveBeenCalledWith('save_glb', expect.anything())
  })

  it('browser preview cannot refine, activate, read saved sources, or export native assets', async () => {
    await expect(refine('asset-1', settings, vi.fn())).rejects.toThrow(/desktop/)
    await expect(readSource('source-1')).rejects.toThrow(/desktop/)
    await expect(saveGeneratedGlb('asset-1', 'asset.glb')).rejects.toThrow(/desktop/)
    await expect(activateLicense('license')).rejects.toThrow(/desktop/)
    expect(await listGenerationJobs()).toEqual([])
    expect(await getAccessStatus()).toMatchObject({ mode: 'preview', canGenerate: false })
    expect(invoke).not.toHaveBeenCalled()
  })

  it('dispatches refinement settings against an existing asset and never starts image generation', async () => {
    vi.mocked(isTauri).mockReturnValue(true)
    const unlisten = vi.fn()
    vi.mocked(listen).mockResolvedValueOnce(unlisten)
    vi.mocked(invoke).mockResolvedValueOnce({ id: 'refined-1', seed: 0, simulated: false, generatedAt: '1700000001000' })
    expect(await refine('original-1', settings, vi.fn())).toMatchObject({ id: 'refined-1', simulated: false, generatedAt: '2023-11-14T22:13:21.000Z' })
    expect(invoke).toHaveBeenCalledExactlyOnceWith('refine_asset', { parentAssetId: 'original-1', settings, jobId: expect.any(String) })
    expect(unlisten).toHaveBeenCalledOnce()
  })

  it('cancels a running refinement and releases its listener even when native reports an error', async () => {
    vi.mocked(isTauri).mockReturnValue(true)
    const unlisten = vi.fn(), controller = new AbortController(), progress = vi.fn()
    let emit!: (event: { payload: GenerationProgress }) => void
    let rejectNative!: (reason: unknown) => void
    vi.mocked(listen).mockImplementationOnce(async (_event, handler) => {
      emit = handler as typeof emit
      return unlisten
    })
    vi.mocked(invoke).mockImplementation((command, args: any) => {
      if (command === 'refine_asset') return new Promise((_resolve, reject) => {
        rejectNative = reject
        emit({ payload: { jobId: args.jobId, stage: 'surface', progress: 40, message: 'Rebuilding surface' } })
      })
      return Promise.resolve(undefined)
    })
    const running = refine('original-1', settings, progress, controller.signal)
    const rejected = expect(running).rejects.toMatchObject({ name: 'AbortError' })
    await vi.waitFor(() => expect(progress).toHaveBeenCalledOnce())
    controller.abort(); rejectNative('Generation cancelled')
    await rejected
    const refineCall = vi.mocked(invoke).mock.calls.find(([command]) => command === 'refine_asset')
    expect(invoke).toHaveBeenCalledWith('cancel_generation', { jobId: (refineCall?.[1] as any).jobId })
    expect(unlisten).toHaveBeenCalledOnce()
  })
})

describe('foreground preparation is independent of trial generation', () => {
  it('returns mask coordinates and identity without treating them as a generated asset', async () => {
    const { prepareMask, saveMask, readMask } = await import('../harness')
    vi.mocked(isTauri).mockReturnValue(true)
    const unlisten = vi.fn(); vi.mocked(listen).mockResolvedValue(unlisten)
    const mask = { sourceId: 'source-id', sha256: 'mask-hash', width: 100, height: 80, imageDataUrl: 'source', maskDataUrl: 'mask' }
    vi.mocked(invoke).mockResolvedValue(mask)
    expect(await prepareMask('source-id', 'auto', () => {})).toEqual(mask)
    expect(invoke).toHaveBeenCalledWith('prepare_mask', expect.objectContaining({ sourceId: 'source-id', background: 'auto' }))
    expect(unlisten).toHaveBeenCalledOnce()
    expect(await saveMask('source-id', 'data:image/png;base64,test')).toEqual(mask)
    expect(await readMask('source-id', 'mask-hash')).toEqual(mask)
    expect(vi.mocked(invoke).mock.calls.some(([command]) => command === 'generate_asset')).toBe(false)
  })
  it('does not pretend masks can be generated from the browser', async () => {
    const { prepareMask } = await import('../harness')
    await expect(prepareMask('source', 'auto', () => {})).rejects.toThrow(/desktop/)
    expect(invoke).not.toHaveBeenCalled()
  })
})
