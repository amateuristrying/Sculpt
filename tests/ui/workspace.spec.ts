import { test, expect, type Page } from '@playwright/test'
import { readFileSync, existsSync } from 'node:fs'
import path from 'node:path'

// IPC is a test double here. Real Metal/process tests live in the Rust harness.
async function nativeBridge(page: Page, installed: boolean) {
  await page.addInitScript(({ installed }) => {
    const host = window as any
    host.isTauri = true
    const callbacks = new Map<number, (event: any) => void>(), events = new Map<string, number>()
    let next = 1, attempt = 0, cancel = false
    const status = { installed, state: installed ? 'ready' : 'missing', engine: 'TripoSR', runtimePath: '/test/runtime', message: 'Local reconstruction on your Mac.', mpsAvailable: true, downloadCacheBytes: 1024 ** 3, recommendedQuality: 'balanced', qualities: [{ id: 'balanced', estimatedMemoryGb: 4.5, recommendedRamGb: 16, resolution: 128 }] }
    const emit = (name: string, payload: any) => callbacks.get(events.get(name)!)?.({ event: name, payload })
    host.__sculptTest = { calls: [] as any[], glb: '', metrics: {}, exported: [] }
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
          case 'install_runtime': {
            attempt++; cancel = false
            emit('sculpt://setup-progress', { jobId: args.jobId, stage: 'models', progress: 50, message: 'Downloading model weights' })
            for (let i = 0; i < 20; i++) { await new Promise(resolve => setTimeout(resolve, 50)); if (cancel) throw new Error('Setup cancelled') }
            if (attempt === 2) throw new Error('Download interrupted. Retry setup.')
            status.installed = true; status.state = 'ready'; return { ...status }
          }
          case 'cancel_generation': cancel = true; return
          case 'clear_download_cache': status.downloadCacheBytes = 0; return { ...status }
          case 'import_source': return { id: 'validated-source', name: args.name, sha256: 'test-digest', mimeType: 'image/png', width: 512, height: 512 }
          case 'generate_asset': return { id: 'generated-asset', seed: 0, simulated: false, generatedAt: String(Date.now()), metrics: host.__sculptTest.metrics }
          case 'read_generated_asset': return host.__sculptTest.glb
          case 'save_glb': host.__sculptTest.exported = args.bytes; return '/test/export.glb'
          default: throw new Error(`Unexpected IPC command: ${command}`)
        }
      },
    }
  }, { installed })
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

test('benchmark meshes load in every view and export unchanged', async ({ page }) => {
  const folder = process.env.SCULPT_BENCHMARK_OUTPUT
  test.skip(!folder || !existsSync(path.join(folder, 'report.json')), 'Set SCULPT_BENCHMARK_OUTPUT to a completed local benchmark')
  const results = JSON.parse(readFileSync(path.join(folder!, 'report.json'), 'utf8')).results
  const errors: string[] = []; page.on('pageerror', error => errors.push(error.message))
  await nativeBridge(page, true); await page.goto('/')
  await page.getByRole('button', { name: 'Open Sculpt' }).click()
  for (const result of results.filter((item: any) => item.validResult)) {
    const request = JSON.parse(readFileSync(path.join(folder!, result.id, 'request.json'), 'utf8'))
    const bytes = readFileSync(path.join(folder!, result.id, 'mesh.glb'))
    await page.evaluate(({ glb, metrics }) => { Object.assign((window as any).__sculptTest, { glb, metrics }) }, { glb: bytes.toString('base64'), metrics: result.metrics })
    await page.locator('input[type=file]').setInputFiles(request.sourcePath)
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
