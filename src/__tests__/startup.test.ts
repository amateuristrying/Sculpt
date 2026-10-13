import { afterEach, describe, expect, it, vi } from 'vitest'
import { canResumeWorkspace, readPreferredEngine, rememberEngine } from '../harness/startup'
import { previewHardware } from '../harness'
import type { BackendStatus, EngineProfile } from '../harness'

const hardware = { ...previewHardware, detectionSource: 'native' as const }
const engine: EngineProfile = { id: 'triposr', name: 'TripoSR', subtitle: '', description: '',
  modelSizeGb: 1.8, runtime: 'PyTorch / Metal', compatibility: 'recommended', reason: '', implemented: true }
const backend: BackendStatus = { installed: true, state: 'ready', mpsAvailable: true, engine: 'TripoSR',
  runtimePath: '', message: '', downloadCacheBytes: 0, recommendedQuality: 'balanced', qualities: [] }

afterEach(() => vi.unstubAllGlobals())

describe('returning desktop startup', () => {
  it('requires a remembered engine and a currently ready compatible native runtime', () => {
    expect(canResumeWorkspace('triposr', hardware, [engine], backend)).toBe(true)
    for (const id of [null, 'demo', 'deleted-engine']) {
      expect(canResumeWorkspace(id, hardware, [engine], backend)).toBe(false)
    }
    expect(canResumeWorkspace('triposr', previewHardware, [engine], backend)).toBe(false)
    expect(canResumeWorkspace('triposr', hardware, [{ ...engine, compatibility: 'unsupported' }], backend)).toBe(false)
    for (const change of [{ installed: false }, { state: 'repair' as const }, { mpsAvailable: false }]) {
      expect(canResumeWorkspace('triposr', hardware, [engine], { ...backend, ...change })).toBe(false)
    }
  })

  it('persists only a versioned preference and tolerates damaged or unavailable storage', () => {
    let value: string | null = null
    vi.stubGlobal('localStorage', { getItem: () => value, setItem: (_key: string, next: string) => { value = next } })
    expect(readPreferredEngine()).toBeNull()
    rememberEngine('triposr')
    expect(readPreferredEngine()).toBe('triposr')
    value = '{broken'
    expect(readPreferredEngine()).toBeNull()
    value = '{"version":2,"engineId":"triposr"}'
    expect(readPreferredEngine()).toBeNull()
    vi.stubGlobal('localStorage', { getItem: () => { throw new Error('Unavailable') }, setItem: () => { throw new Error('Full') } })
    expect(readPreferredEngine()).toBeNull()
    expect(() => rememberEngine('triposr')).not.toThrow()
  })
})
