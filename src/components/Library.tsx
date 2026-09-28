import { ArrowRight, Check, RotateCcw } from 'lucide-react'
import type { GenerationJob } from '../harness'

export default function Library({ jobs, error, busy, onRefresh, onOpen }: {
  jobs: GenerationJob[]; error: string | null; busy: boolean
  onRefresh: () => void; onOpen: (job: GenerationJob) => void
}) {
  return <>
    <p className="dialog-description">Your images and reconstructed assets are saved on this Mac. Reopen an asset or retry a previous source.</p>
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
    <p className="library-footnote">The library keeps the latest 50 jobs in this view. Local generation is free. Reopen saved assets to inspect, refine, or export them.</p>
  </>
}
