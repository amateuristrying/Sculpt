import { Check, Download, LoaderCircle, RotateCcw } from 'lucide-react'
import type { BackendStatus, SetupProgress } from '../harness'

export interface RuntimeSetupProps {
  backend: BackendStatus | null
  progress: SetupProgress | null
  busy: boolean
  error: string | null
  canInstall: boolean
  onInstall(): void
  onCancel(): void
  onClearCache(): void
}

export default function RuntimeSetup({ backend, progress, busy, error, canInstall, onInstall, onCancel, onClearCache }: RuntimeSetupProps) {
  return <section className="runtime-setup" aria-label="Local runtime setup">
    <div className="runtime-heading">{busy ? <LoaderCircle className="spin" size={16} /> : backend?.installed ? <Check size={16} /> : <Download size={16} />}<strong>{busy ? 'Preparing your local engine' : backend?.installed ? 'Local engine ready' : backend?.state === 'repair' ? 'Repair your local engine' : 'Set up local reconstruction'}</strong></div>
    <p>{busy ? progress?.message || 'Starting setup…' : backend?.message || 'Checking your local runtime…'}</p>
    {busy && <><div className="progress-track"><span style={{ width: `${progress?.progress || 0}%` }} /></div><div className="runtime-actions"><span>Setup stages · {Math.round(progress?.progress || 0)}%</span><button className="text-button" onClick={onCancel}>Cancel setup</button></div></>}
    {error && <p role="alert" className="runtime-error">{error}</p>}
    {!busy && canInstall && <div className="runtime-actions"><span>{backend?.installed ? 'Model weights verified' : 'First setup downloads about 2 GB'}</span><button className={backend?.installed ? 'text-button' : 'secondary-button'} onClick={onInstall}>{backend?.installed ? <RotateCcw size={12} /> : <Download size={13} />}{backend?.installed ? 'Verify / repair' : backend?.state === 'repair' ? 'Repair runtime' : 'Install local engine'}</button></div>}
    {!busy && canInstall && !!backend?.downloadCacheBytes && <div className="runtime-actions"><span>{(backend.downloadCacheBytes / 1024 ** 3).toFixed(1)} GB reusable downloads</span><button className="text-button" onClick={onClearCache} title="Remove cached packages. Installed models and offline generation stay available.">Clear downloads</button></div>}
  </section>
}
