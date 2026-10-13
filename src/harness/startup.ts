import type { BackendStatus, EngineProfile, HardwareProfile } from './types'

const KEY = 'sculpt.workspace.v1'

export function readPreferredEngine(): string | null {
  try {
    const saved = JSON.parse(localStorage.getItem(KEY) ?? 'null')
    return saved?.version === 1 && typeof saved.engineId === 'string' ? saved.engineId : null
  } catch { return null }
}

export function rememberEngine(engineId: string): void {
  // A full or unavailable webview store must never stop someone opening an asset.
  try { localStorage.setItem(KEY, JSON.stringify({ version: 1, engineId })) } catch { /* optional preference */ }
}

export function canResumeWorkspace(engineId: string | null, hardware: HardwareProfile,
  engines: EngineProfile[], backend: BackendStatus): boolean {
  return hardware.detectionSource === 'native' && backend.installed && backend.state === 'ready'
    && backend.mpsAvailable && engines.some(engine => engine.id === engineId && engine.id !== 'demo'
      && engine.implemented && engine.compatibility !== 'unsupported')
}
