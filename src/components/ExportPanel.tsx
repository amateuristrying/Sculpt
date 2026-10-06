import { useState } from 'react'
import { ArrowRight, Box, Check, Download, Info, LoaderCircle } from 'lucide-react'
import type { AssetMetadata } from './Viewport'

export type ExportFormat = 'glb' | 'stl'

export default function ExportPanel({ name, onName, onExport, busy, ready, savedAsset, metadata }: {
  name: string; onName: (name: string) => void
  onExport: (format: ExportFormat, heightMm: number) => void
  busy: boolean; ready: boolean; savedAsset: boolean; metadata: AssetMetadata | null
}) {
  const [format, setFormat] = useState<ExportFormat>('glb')
  const [height, setHeight] = useState('100')
  const heightMm = Number(height)
  const validHeight = Number.isFinite(heightMm) && heightMm >= 0.1 && heightMm <= 10_000
  return <>
    <span className="dialog-kicker">EXPORT ASSET</span>
    <p className="dialog-description">Take your geometry wherever you create next.</p>
    <div className="export-format-options">
      {(['glb', 'stl'] as const).map(option => <button key={option} aria-pressed={format === option}
        className={format === option ? 'selected' : ''} disabled={busy || (option === 'stl' && !savedAsset)}
        onClick={() => setFormat(option)}>
        <Box size={20} /><strong>{option.toUpperCase()}</strong>
        <small>{option === 'glb' ? 'Geometry + material' : savedAsset ? 'Geometry for printing' : 'Saved desktop assets'}</small>
        {format === option && <Check size={13} />}
      </button>)}
    </div>
    <label className="field-label" htmlFor="export-name">File name</label>
    <div className="export-name"><input id="export-name" value={name} disabled={busy} onChange={event => onName(event.target.value)} /><span>.{format}</span></div>
    {format === 'stl' && <>
      <label className="field-label" htmlFor="export-height">Object height (mm)</label>
      <div className="export-name"><input id="export-height" type="number" min="0.1" max="10000" step="any"
        value={height} disabled={busy} aria-invalid={!validHeight} aria-describedby="export-scale-note"
        onChange={event => setHeight(event.target.value)} /><span>mm</span></div>
      <p id="export-scale-note" className="export-scale-note">{validHeight ? 'Proportions are preserved. Z-up, centered on the bed, with its base at zero.' : 'Enter a height between 0.1 and 10,000 mm.'}</p>
    </>}
    <div className="export-summary"><span>{metadata?.faces.toLocaleString()} faces</span>
      <span>{format === 'stl' ? 'Millimetres' : metadata?.textureResolution ?? 'No textures'}</span>
    </div>
    <div className="dialog-note"><Info size={14} /><span>{format === 'stl'
      ? 'STL contains geometry only. Scaling does not repair holes or disconnected parts; check the mesh in your slicer before printing.'
      : savedAsset ? 'Exports the saved geometry, material and texture. Viewport display changes are excluded.' : 'This exports a procedural sample, not an AI reconstruction.'}</span></div>
    <button className="primary-button full-width" disabled={busy || !ready || (format === 'stl' && !validHeight)}
      onClick={() => onExport(format, heightMm)}>
      {busy ? <LoaderCircle className="spin" size={16} /> : <Download size={16} />}
      {busy ? 'Saving asset…' : `Export ${format.toUpperCase()}`}<ArrowRight size={14} />
    </button>
  </>
}
