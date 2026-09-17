import { useRef, useState } from 'react'
import { ArrowRight, Check, FileKey2, RotateCcw } from 'lucide-react'
import { activateLicense } from '../harness'
import type { AccessStatus, GenerationJob } from '../harness'

export default function Library({ jobs, access, error, busy, onRefresh, onOpen, onActivated }: {
  jobs: GenerationJob[]; access: AccessStatus | null; error: string | null; busy: boolean
  onRefresh: () => void; onOpen: (job: GenerationJob) => void; onActivated: (status: AccessStatus) => void
}) {
  const input = useRef<HTMLInputElement>(null)
  const [activationError, setActivationError] = useState<string | null>(null)
  const [activating, setActivating] = useState(false)
  const activate = async (file?: File) => {
    if (!file || activating) return
    setActivationError(null); setActivating(true)
    try {
      if (file.size > 16 * 1024) throw new Error('Choose a Sculpt license file smaller than 16 KB.')
      onActivated(await activateLicense(await file.text()))
    } catch (error) { setActivationError(String(error instanceof Error ? error.message : error)) }
    finally { setActivating(false) }
  }
  return <>
    <p className="dialog-description">Your images and reconstructed assets are saved on this Mac. Reopen an asset or retry a previous source.</p>
    <div className="library-access">
      <strong>{access?.mode === 'paid' ? 'Sculpt activated' : access?.mode === 'development' ? 'Development build' : 'Sculpt trial'}</strong>
      <p>{access?.message || 'Checking local access…'}</p>
      {access?.activationAvailable && access.mode !== 'paid' && <>
        <input ref={input} type="file" accept=".json,.sculpt-license" hidden aria-label="License file" onChange={event => { void activate(event.target.files?.[0]); event.target.value = '' }} />
        <button className="text-button" onClick={() => input.current?.click()} disabled={activating}><FileKey2 size={13} /> {activating ? 'Verifying license…' : 'Import license file'}</button>
      </>}
      {activationError && <p role="alert">{activationError}</p>}
    </div>
    <div className="library-heading"><span className="section-label">RECENT GENERATIONS</span><button className="icon-button" onClick={onRefresh} disabled={busy} aria-label="Refresh library"><RotateCcw size={14} /></button></div>
    {error && <div className="generation-error" role="alert">{error}</div>}
    <div className="library-jobs">
      {!jobs.length && <p className="library-empty">{busy ? 'Opening local library…' : 'Generate your first asset to start your library.'}</p>}
      {jobs.map(job => <div className="library-job" key={job.id}>
        <div><strong>{job.request.imageName}</strong><span>{job.state === 'succeeded' && <Check size={11} />} {job.state} · {job.request.geometry} · {new Date(job.createdAt).toLocaleString()}</span>{job.error && <p>{job.error}</p>}</div>
        <button className="text-button" disabled={busy || job.state === 'running' || (!job.asset && !job.request.sourceId)} onClick={() => onOpen(job)} aria-label={`${job.state === 'succeeded' ? 'Open' : 'Retry source'} ${job.request.imageName}`}>
          {job.state === 'succeeded' ? 'Open' : 'Retry source'} <ArrowRight size={12} />
        </button>
      </div>)}
    </div>
    <p className="library-footnote">The library keeps the latest 50 jobs in this view. Failed and cancelled jobs do not use your free reconstruction. Existing assets remain available to inspect and export.</p>
  </>
}
