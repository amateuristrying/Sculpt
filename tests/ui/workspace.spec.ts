import { test, expect, type Page } from '@playwright/test'
import { readFileSync, existsSync } from 'node:fs'
import path from 'node:path'

// Authored tetrahedron fixture: real, self-contained GLB bytes, no model weights
// or personal photo needed. Neural reconstruction is tested in the native suite.
function fixtureGlb(): number[] {
  const points = [[-.8, -.6, -.5], [.8, -.6, -.5], [0, .85, 0], [0, -.6, .8]]
  const faces = [[0, 2, 1], [0, 1, 3], [1, 2, 3], [2, 0, 3]]
  const positions: number[] = [], normals: number[] = []
  for (const face of faces) {
    const [a, b, c] = face.map(index => points[index])
    const u = b.map((value, i) => value - a[i]), v = c.map((value, i) => value - a[i])
    const normal = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]]
    const length = Math.hypot(...normal)
    for (const index of face) { positions.push(...points[index]); normals.push(...normal.map(value => value / length)) }
  }
  const binary = Buffer.concat([Buffer.from(new Float32Array(positions).buffer), Buffer.from(new Float32Array(normals).buffer)])
  const document = { asset: { version: '2.0', generator: 'Sculpt UI test fixture' }, scene: 0, scenes: [{ nodes: [0] }], nodes: [{ mesh: 0 }],
    meshes: [{ primitives: [{ attributes: { POSITION: 0, NORMAL: 1 }, material: 0 }] }],
    materials: [{ pbrMetallicRoughness: { baseColorFactor: [.95, .62, .14, 1], metallicFactor: 0, roughnessFactor: .8 } }],
    buffers: [{ byteLength: binary.length }], bufferViews: [{ buffer: 0, byteOffset: 0, byteLength: 144 }, { buffer: 0, byteOffset: 144, byteLength: 144 }],
    accessors: [{ bufferView: 0, componentType: 5126, count: 12, type: 'VEC3', min: [-.8, -.6, -.5], max: [.8, .85, .8] }, { bufferView: 1, componentType: 5126, count: 12, type: 'VEC3' }] }
  let json = Buffer.from(JSON.stringify(document)); json = Buffer.concat([json, Buffer.alloc((4 - json.length % 4) % 4, 32)])
  const header = Buffer.alloc(20); header.write('glTF'); header.writeUInt32LE(2, 4); header.writeUInt32LE(28 + json.length + binary.length, 8); header.writeUInt32LE(json.length, 12); header.write('JSON', 16)
  const binHeader = Buffer.alloc(8); binHeader.writeUInt32LE(binary.length); binHeader.write('BIN\0', 4)
  return [...Buffer.concat([header, json, binHeader, binary])]
}

async function imageFixture(page: Page, name = 'tetrahedron.png') {
  const base64 = await page.evaluate(() => {
    const canvas = document.createElement('canvas'); canvas.width = canvas.height = 64
    const context = canvas.getContext('2d')!; context.fillStyle = '#e9a535'; context.fillRect(0, 0, 64, 64)
    return canvas.toDataURL().split(',')[1]
  })
  return { name, mimeType: 'image/png', buffer: Buffer.from(base64, 'base64') }
}

test('browser photo import cannot be mistaken for AI reconstruction', async ({ page }) => {
  await page.goto('/')
  await page.getByRole('button', { name: 'Open Sculpt' }).click()
  await page.getByRole('button', { name: 'Explore the demo' }).click()
  await expect(page.getByRole('button', { name: 'Generate demo', exact: true })).toBeEnabled()
  const data = await page.evaluate(() => {
    const canvas = document.createElement('canvas'); canvas.width = canvas.height = 64
    const context = canvas.getContext('2d')!; context.fillStyle = '#ffd600'; context.fillRect(0, 0, 64, 64)
    return canvas.toDataURL().split(',')[1]
  })
  await page.locator('input[type=file]').setInputFiles({ name: 'user-photo.png', mimeType: 'image/png', buffer: Buffer.from(data, 'base64') })
  await expect(page.getByText('Desktop app required', { exact: true })).toBeVisible()
  await expect(page.getByRole('button', { name: 'Reconstruction unavailable' })).toBeDisabled()
  await expect(page.locator('.viewport-title-tag')).toHaveText('SAMPLE ASSET')
  await expect(page.getByRole('button', { name: 'Generate demo', exact: true })).toHaveCount(0)
})

// IPC is a test double here. Real Metal/process tests live in the Rust harness.
async function nativeBridge(page: Page, installed: boolean, trial = false) {
  await page.addInitScript(({ installed, trial, fixture }) => {
    const host = window as any
    host.isTauri = true
    const callbacks = new Map<number, (event: any) => void>(), events = new Map<string, number>()
    let next = 1, attempt = 0, cancel = false
    const status = { installed, state: installed ? 'ready' : 'missing', engine: 'TripoSR', runtimePath: '/test/runtime', message: 'Local reconstruction on your Mac.', mpsAvailable: true, downloadCacheBytes: 1024 ** 3, recommendedQuality: 'balanced', qualities: [{ id: 'balanced', estimatedMemoryGb: 4.5, recommendedRamGb: 16, resolution: 128 }] }
    const emit = (name: string, payload: any) => callbacks.get(events.get(name)!)?.({ event: name, payload })
    const saved = JSON.parse(localStorage.getItem('sculpt-test-library') || '{"jobs":[],"sources":{},"trialUsed":false}')
    saved.masks ||= {}
    const assets = new Map<string, number[]>()
    // Browser quota is not the native library's disk capacity. Keep real large
    // benchmark sources in this test process; only tiny fixtures need reloads.
    const persist = () => localStorage.setItem('sculpt-test-library', JSON.stringify({ ...saved,
      sources: Object.fromEntries(Object.entries(saved.sources).filter(([, value]) => (value as any).dataUrl.length < 64 * 1024)) }))
    const access = () => ({ mode: trial ? 'trial' : 'development', canGenerate: !trial || !saved.trialUsed, freeGenerationsRemaining: trial ? Number(!saved.trialUsed) : null,
      activationAvailable: false, message: trial && saved.trialUsed ? 'Your free reconstruction is complete. Activate Sculpt to continue generating locally.' : 'Local reconstruction available.' })
    host.__sculptTest = { calls: [] as any[], glb: fixture, metrics: { faces: 4, vertices: 12, totalSeconds: 1, device: 'mps', canRefine: true, resolution: 128 }, exported: [], saved,
      refinementError: null as string | null, holdRefinement: false }
    const complete = (jobId: string, request: any, refined: boolean) => {
      const asset = { id: jobId, seed: 0, simulated: false, generatedAt: String(Date.now()), metrics: { ...host.__sculptTest.metrics, ...(refined ? { refinement: { ...request.refinement } } : {}), operation: refined ? 'refine' : 'generate' } }
      assets.set(jobId, [...host.__sculptTest.glb])
      saved.jobs.unshift({ id: jobId, request, state: 'succeeded', createdAt: asset.generatedAt, updatedAt: asset.generatedAt, error: null, asset })
      if (!refined) saved.trialUsed = true
      persist(); return asset
    }
    host.__TAURI_EVENT_PLUGIN_INTERNALS__ = { unregisterListener: (_: string, id: number) => callbacks.delete(id) }
    host.__TAURI_INTERNALS__ = {
      transformCallback(callback: any) { const id = next++; callbacks.set(id, callback); return id },
      async invoke(command: string, args: any) {
        host.__sculptTest.calls.push({ command, args })
        switch (command) {
          case 'plugin:event|listen': events.set(args.event, args.handler); return args.handler
          case 'plugin:event|unlisten': return
          case 'detect_hardware': return { os: 'macOS', osVersion: 'Test fixture', chip: 'Apple M4', gpu: 'Apple M4', memoryGb: 16, architecture: 'arm64', isAppleSilicon: true, unifiedMemory: true, detectionSource: 'native', storageGb: 256, computeBackends: ['Metal', 'CPU'] }
          case 'get_engines': return [{ id: 'triposr', name: 'TripoSR', subtitle: 'Local reconstruction', description: 'On-device geometry', modelSizeGb: 1.8, runtime: 'PyTorch / Metal', compatibility: 'recommended', reason: 'Local inference', implemented: true }, { id: 'demo', name: 'Workspace Demo', subtitle: 'No AI', description: 'Procedural sample', modelSizeGb: null, runtime: 'Mock', compatibility: 'available', reason: 'No reconstruction', implemented: true }]
          case 'backend_status': return { ...status }
          case 'list_generation_jobs': return [...saved.jobs]
          case 'get_access_status': return access()
          case 'install_runtime': {
            attempt++; cancel = false
            emit('sculpt://setup-progress', { jobId: args.jobId, stage: 'models', progress: 50, message: 'Downloading model weights' })
            for (let i = 0; i < 20; i++) { await new Promise(resolve => setTimeout(resolve, 50)); if (cancel) throw new Error('Setup cancelled') }
            if (attempt === 2) throw new Error('Download interrupted. Retry setup.')
            status.installed = true; status.state = 'ready'; return { ...status }
          }
          case 'cancel_generation': cancel = true; return
          case 'clear_download_cache': status.downloadCacheBytes = 0; return { ...status }
          case 'import_source': {
            const asset = { id: 'validated-source', name: args.name, sha256: 'test-digest', mimeType: 'image/png', width: 64, height: 64 }
            saved.sources[asset.id] = { asset, dataUrl: args.dataUrl, size: 1024 }; persist(); return asset
          }
          case 'read_source': {
            const source = saved.sources[args.sourceId]; if (!source) throw new Error('Source unavailable'); return source
          }
          case 'prepare_mask': {
            const source = saved.sources[args.sourceId]
            const image = new Image(); image.src = source.dataUrl; await image.decode()
            const canvas = document.createElement('canvas'); canvas.width = image.width; canvas.height = image.height
            const ctx = canvas.getContext('2d')!; ctx.fillStyle = '#fff'; ctx.fillRect(0, 0, canvas.width, canvas.height)
            return { sourceId: args.sourceId, sha256: 'automatic-mask', width: image.width, height: image.height,
              imageDataUrl: source.dataUrl, maskDataUrl: canvas.toDataURL() }
          }
          case 'save_mask': {
            const image = new Image(); image.src = args.dataUrl; await image.decode()
            const mask = { sourceId: args.sourceId, sha256: 'approved-mask', width: image.width, height: image.height,
              imageDataUrl: saved.sources[args.sourceId].dataUrl, maskDataUrl: args.dataUrl }
            saved.masks[args.sourceId] = mask; persist(); return mask
          }
          case 'read_mask': return saved.masks[args.sourceId]
          case 'generate_asset': {
            if (!access().canGenerate) throw new Error('Trial already used')
            return complete(args.jobId, args.request, false)
          }
          case 'refine_asset': {
            const parent = saved.jobs.find((job: any) => job.id === args.parentAssetId)
            if (!parent) throw new Error('Unknown parent asset')
            if (host.__sculptTest.refinementError) throw new Error(host.__sculptTest.refinementError)
            cancel = false
            emit('sculpt://generation-progress', { jobId: args.jobId, stage: 'surface', progress: 55, message: 'Refining saved geometry' })
            while (host.__sculptTest.holdRefinement) {
              await new Promise(resolve => setTimeout(resolve, 25))
              if (cancel) throw new Error('Refinement cancelled')
            }
            return complete(args.jobId, { ...parent.request, parentAssetId: parent.id, refinement: args.settings }, true)
          }
          case 'read_generated_asset': {
            if (!saved.jobs.some((job: any) => job.id === args.assetId)) throw new Error('Asset unavailable')
            return Uint8Array.from(assets.get(args.assetId) || fixture).buffer
          }
          case 'save_generated_glb': {
            if (!saved.jobs.some((job: any) => job.id === args.assetId)) throw new Error('Asset unavailable')
            host.__sculptTest.exported = [...(assets.get(args.assetId) || fixture)]; return '/test/export.glb'
          }
          case 'save_glb': host.__sculptTest.exported = args.bytes; return '/test/export.glb'
          default: throw new Error(`Unexpected IPC command: ${command}`)
        }
      },
    }
  }, { installed, trial, fixture: fixtureGlb() })
}

test('installer cancellation, failed download, retry, and cache clearing', async ({ page }) => {
  await nativeBridge(page, false); await page.goto('/')
  await page.getByRole('button', { name: 'Install local engine' }).click()
  await expect(page.getByText('Downloading model weights')).toBeVisible()
  await page.getByRole('button', { name: 'Cancel setup' }).click()
  await expect(page.getByRole('alert')).toContainText('Setup cancelled')
  await page.getByRole('button', { name: 'Install local engine' }).click()
  await expect(page.getByRole('alert')).toContainText('Download interrupted')
  await page.getByRole('button', { name: 'Install local engine' }).click()
  await expect(page.getByText('Local engine ready', { exact: true })).toBeVisible()
  await page.getByRole('button', { name: 'Clear downloads' }).click()
  await expect(page.getByRole('button', { name: 'Clear downloads' })).toHaveCount(0)
  await expect(page.getByRole('button', { name: 'Verify / repair' })).toBeVisible()
  await page.screenshot({ path: 'test-results/runtime-ready.png', fullPage: true })
})

test('trial asset can be refined, reopened after reload, and exported without another generation', async ({ page }) => {
  const errors: string[] = []; page.on('pageerror', error => errors.push(error.message))
  await nativeBridge(page, true, true); await page.goto('/')
  await page.getByRole('button', { name: 'Open Sculpt', exact: true }).click()
  await page.locator('input[type=file]').setInputFiles(await imageFixture(page))
  await approveMask(page)
  await page.getByRole('button', { name: 'Generate 3D', exact: true }).click()
  await expect(page.locator('.viewport-title-tag')).toHaveText('RECONSTRUCTED')
  await expect(page.getByRole('button', { name: 'Generate again', exact: true })).toBeDisabled()
  const original = await page.evaluate(() => (window as any).__sculptTest.saved.jobs[0].id)

  // This fixture makes the normal CI exercise all rendering modes without weights.
  const pixels = new Set<string>()
  for (const mode of ['Material', 'Wireframe', 'Points', 'Technical']) {
    await page.getByRole('group', { name: 'Visualization mode' }).getByRole('button', { name: mode, exact: true }).click()
    await page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))))
    pixels.add(await page.locator('canvas').evaluate((canvas: HTMLCanvasElement) => canvas.toDataURL()))
  }
  expect(pixels.size).toBe(4)
  await page.getByLabel('Extraction resolution').selectOption('256')
  await page.getByLabel('Surface density').fill('20')
  await page.getByLabel('Smoothing iterations').fill('3')
  await page.getByLabel('Remove tiny fragments').check()
  await page.getByRole('button', { name: 'Apply refinement' }).click()
  await expect(page.getByText('Refined version saved', { exact: true })).toBeVisible()
  const saved = await page.evaluate(() => (window as any).__sculptTest.saved)
  expect(saved.trialUsed).toBe(true)
  expect(saved.jobs).toHaveLength(2)
  expect(saved.jobs[1].id).toBe(original)
  expect(saved.jobs[0].request).toMatchObject({ parentAssetId: original, refinement: { resolution: 256, densityThreshold: 20, smoothingIterations: 3, removeSmallComponents: true } })
  await expect(page.getByLabel('Extraction resolution')).toHaveValue('256')
  await expect(page.getByRole('button', { name: 'Generate again', exact: true })).toBeDisabled()
  await page.screenshot({ path: 'test-results/refine-trial.png', fullPage: true })

  await page.reload()
  await page.getByRole('button', { name: 'Open Sculpt', exact: true }).click()
  await page.getByRole('button', { name: /Local library/ }).click()
  await expect(page.getByRole('button', { name: 'Open tetrahedron.png', exact: true })).toHaveCount(2)
  // Reopen the original: refinement must not replace or remove it.
  await page.getByRole('button', { name: 'Open tetrahedron.png', exact: true }).last().click()
  await expect(page.locator('.viewport-title-tag')).toHaveText('RECONSTRUCTED')
  await page.getByRole('button', { name: 'Export', exact: true }).last().click()
  await page.getByRole('button', { name: 'Export GLB', exact: true }).click()
  const state = await page.evaluate(() => (window as any).__sculptTest)
  expect(state.exported).toEqual(fixtureGlb())
  expect(state.calls.findLast((call: any) => call.command === 'save_generated_glb').args).toMatchObject({ assetId: original })
  expect(state.calls.some((call: any) => call.command === 'generate_asset' || call.command === 'save_glb')).toBe(false)
  expect(errors).toEqual([])
})

test('failed and cancelled refinements preserve the saved asset and can be retried', async ({ page }) => {
  await nativeBridge(page, true, true); await page.goto('/')
  await page.getByRole('button', { name: 'Open Sculpt', exact: true }).click()
  await page.locator('input[type=file]').setInputFiles(await imageFixture(page))
  await approveMask(page)
  await page.getByRole('button', { name: 'Generate 3D', exact: true }).click()
  await expect(page.locator('.viewport-title-tag')).toHaveText('RECONSTRUCTED')
  const original = await page.evaluate(() => (window as any).__sculptTest.saved.jobs[0].id)
  await page.evaluate(() => { (window as any).__sculptTest.refinementError = 'The refinement cache is damaged.' })
  await page.getByRole('button', { name: 'Apply refinement' }).click()
  await expect(page.getByRole('alert')).toContainText('cache is damaged')
  await expect(page.locator('.viewport-title-tag')).toHaveText('RECONSTRUCTED')
  await page.evaluate(() => { Object.assign((window as any).__sculptTest, { refinementError: null, holdRefinement: true }) })
  await page.getByRole('button', { name: 'Apply refinement' }).click()
  await expect(page.getByText('Refining saved geometry', { exact: true })).toBeVisible()
  await page.getByRole('button', { name: 'Cancel', exact: true }).click()
  await expect(page.getByRole('button', { name: 'Apply refinement' })).toBeEnabled()
  expect(await page.evaluate(() => (window as any).__sculptTest.saved.jobs.map((job: any) => job.id))).toEqual([original])
  await page.evaluate(() => { (window as any).__sculptTest.holdRefinement = false })
  await page.getByRole('button', { name: 'Apply refinement' }).click()
  await expect(page.getByText('Refined version saved', { exact: true })).toBeVisible()
  expect(await page.evaluate(() => (window as any).__sculptTest.saved.jobs.length)).toBe(2)
})

test('benchmark meshes load in every view and export unchanged', async ({ page }) => {
  const folder = process.env.SCULPT_BENCHMARK_OUTPUT
  test.skip(!folder || !existsSync(path.join(folder, 'report.json')), 'Set SCULPT_BENCHMARK_OUTPUT to a completed local benchmark')
  const results = JSON.parse(readFileSync(path.join(folder!, 'report.json'), 'utf8')).results
  const successful = results.filter((item: any) => item.validResult)
  expect(successful.length).toBeGreaterThan(0)
  // The real-photo set is much larger than the original six showcase fixtures.
  // Keep a bounded per-asset allowance for five screenshots and byte-exact export.
  test.setTimeout(Math.max(120_000, successful.length * 20_000))
  const errors: string[] = []; page.on('pageerror', error => errors.push(error.message))
  await nativeBridge(page, true); await page.goto('/')
  await page.getByRole('button', { name: 'Open Sculpt' }).click()
  for (const result of successful) {
    const request = JSON.parse(readFileSync(path.join(folder!, result.id, 'request.json'), 'utf8'))
    const bytes = readFileSync(path.join(folder!, result.id, 'mesh.glb'))
    await page.evaluate(({ glb, metrics }) => { Object.assign((window as any).__sculptTest, { glb, metrics }) }, { glb: [...bytes], metrics: result.metrics })
    await page.locator('input[type=file]').setInputFiles(request.sourcePath)
    await expect(page.getByRole('button', { name: 'Generate 3D', exact: true })).toBeEnabled()
    await approveMask(page)
  await page.getByRole('button', { name: 'Generate 3D', exact: true }).click()
    await expect(page.locator('.viewport-title-tag')).toHaveText('RECONSTRUCTED')
    await expect(page.locator('.asset-metadata')).toContainText(result.metrics.faces.toLocaleString('en-US'))
    const pixels = new Set<string>()
    for (const mode of ['Material', 'Wireframe', 'Points', 'Technical']) {
      await page.getByRole('group', { name: 'Visualization mode' }).getByRole('button', { name: mode, exact: true }).click()
      await page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))))
      pixels.add(await page.locator('canvas').evaluate((canvas: HTMLCanvasElement) => canvas.toDataURL()))
      await page.locator('.viewport-stage').screenshot({ path: `test-results/${result.id}-${mode.toLowerCase()}.png` })
    }
    expect(pixels.size).toBe(4)
    await page.getByRole('group', { name: 'Visualization mode' }).getByRole('button', { name: 'Material', exact: true }).click()
    const canvas = await page.locator('canvas').boundingBox()
    await page.mouse.move(canvas!.x + canvas!.width * 0.5, canvas!.y + canvas!.height * 0.5)
    await page.mouse.down(); await page.mouse.move(canvas!.x + canvas!.width * 0.85, canvas!.y + canvas!.height * 0.6, { steps: 20 }); await page.mouse.up()
    await page.locator('.viewport-stage').screenshot({ path: `test-results/${result.id}-reverse.png` })
    await page.getByRole('button', { name: 'Export', exact: true }).last().click()
    await page.getByRole('button', { name: 'Export GLB', exact: true }).click()
    expect(Buffer.from(await page.evaluate(() => (window as any).__sculptTest.exported))).toEqual(bytes)
    const calls = await page.evaluate(() => (window as any).__sculptTest.calls)
    expect(calls.findLast((call: any) => call.command === 'generate_asset').args.request).toMatchObject({ sourceId: 'validated-source', engineId: 'triposr', background: 'auto' })
    expect(calls.findLast((call: any) => call.command === 'import_source').args.dataUrl).toContain(';base64,')
    await page.getByRole('button', { name: 'Reset camera (F)' }).click()
  }
  expect(errors).toEqual([])
})

async function approveMask(page: Page) {
  await page.locator('.generate-button').click()
  await page.getByRole('button', { name: 'Use this mask', exact: true }).click()
}

test('foreground brush corrections are saved with generation and restored from the library', async ({ page }) => {
  await nativeBridge(page, true, true); await page.goto('/')
  await page.getByRole('button', { name: 'Open Sculpt', exact: true }).click()
  await page.locator('input[type=file]').setInputFiles(await imageFixture(page, 'mask-edit.png'))
  await page.locator('.generate-button').click()
  const dialog = page.getByRole('dialog', { name: 'Keep only your object.' })
  await expect(dialog.getByRole('button', { name: 'Use this mask' })).toBeEnabled()
  const canvas = dialog.locator('canvas')
  const before = await canvas.evaluate((c: HTMLCanvasElement) => c.toDataURL())
  const rect = (await canvas.boundingBox())!
  await canvas.click({ position: { x: rect.width / 4, y: rect.height / 2 } })
  expect(await canvas.evaluate((c: HTMLCanvasElement) => c.toDataURL())).not.toBe(before)
  await dialog.getByRole('button', { name: 'Undo', exact: true }).click()
  expect(await canvas.evaluate((c: HTMLCanvasElement) => c.toDataURL())).toBe(before)
  await canvas.click({ position: { x: rect.width / 4, y: rect.height / 2 } })
  await dialog.getByRole('button', { name: 'Add', exact: true }).click()
  await dialog.getByLabel('Brush size').fill('6')
  await canvas.click({ position: { x: rect.width / 4, y: rect.height / 2 } })
  await dialog.getByRole('button', { name: 'Cutout', exact: true }).click()
  await page.screenshot({ path: 'test-results/mask-editor.png', fullPage: true })
  await dialog.getByRole('button', { name: 'Use this mask' }).click()
  expect(await page.evaluate(() => (window as any).__sculptTest.saved.trialUsed)).toBe(false)
  expect(await page.evaluate(() => (window as any).__sculptTest.saved.jobs)).toHaveLength(0)
  await page.getByRole('button', { name: 'Generate 3D', exact: true }).click()
  await expect(page.locator('.viewport-title-tag')).toHaveText('RECONSTRUCTED')
  expect(await page.evaluate(() => (window as any).__sculptTest.saved.jobs[0].request.maskSha256)).toBe('approved-mask')
  await page.reload(); await page.getByRole('button', { name: 'Open Sculpt', exact: true }).click()
  await page.getByRole('button', { name: /Local library/ }).click()
  await page.getByRole('button', { name: 'Open mask-edit.png', exact: true }).click()
  await expect(page.getByRole('button', { name: /Edit foreground mask/ })).toBeVisible()
  await page.getByRole('button', { name: /Edit foreground mask/ }).click()
  await expect(page.getByRole('dialog', { name: 'Keep only your object.' })).toBeVisible()
  expect(await page.evaluate(() => (window as any).__sculptTest.calls.filter((c: any) => c.command === 'prepare_mask'))).toHaveLength(0)
})
