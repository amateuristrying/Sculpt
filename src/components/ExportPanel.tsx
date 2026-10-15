import { useState } from 'react'
import { ArrowRight, Box, Check, Download, Info, LoaderCircle } from 'lucide-react'
import type { AssetMetadata } from './Viewport'

export type ExportFormat = 'glb' | 'gltf' | 'obj' | 'stl'

const formats: { id: ExportFormat; label: string; detail: string; extension: string }[] = [
  { id: 'glb', label: 'GLB', detail: 'Geometry + material', extension: 'glb' },
  { id: 'gltf', label: 'glTF', detail: 'Single embedded file', extension: 'gltf' },
  { id: 'obj', label: 'OBJ', detail: 'Mesh + MTL + textures', extension: 'zip' },
  { id: 'stl', label: 'STL', detail: 'Geometry for printing', extension: 'stl' },
]

export default function ExportPanel({ name, onName, onExport, busy, ready, savedAsset, metadata }: {
  name: string; onName: (name: string) => void
  onExport: (format: ExportFormat, heightMm: number) => void
  busy: boolean; ready: boolean; savedAsset: boolean; metadata: AssetMetadata | null
}) {
  const [format, setFormat] = useState<ExportFormat>('glb')
  const [height, setHeight] = useState('100')
  const heightMm = Number(height)
  const validHeight = Number.isFinite(heightMm) && heightMm >= 0.1 && heightMm <= 10_000
  const selected = formats.find(option => option.id === format)!
  return <>
    <span className="dialog-kicker">EXPORT ASSET</span>
    <p className="dialog-description">Take your geometry wherever you create next.</p>
    <div className="export-format-options">
      {formats.map(option => <button key={option.id} aria-pressed={format === option.id}
        className={format === option.id ? 'selected' : ''} disabled={busy || (option.id !== 'glb' && !savedAsset)}
        onClick={() => setFormat(option.id)}>
        <Box size={20} /><strong>{option.label}</strong>
        <small>{option.id === 'glb' || savedAsset ? option.detail : 'Saved desktop assets'}</small>
        {format === option.id && <Check size={13} />}
      </button>)}
    </div>
    <label className="field-label" htmlFor="export-name">File name</label>
    <div className="export-name"><input id="export-name" value={name} disabled={busy} onChange={event => onName(event.target.value)} /><span>.{selected.extension}</span></div>
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
      : format === 'obj' ? 'Unzip before importing model.obj. Keep model.mtl and PNG textures beside it. Vertex colors depend on importer support; bake a texture in Refine for portable color. Full PBR material settings require GLB or glTF.'
      : format === 'gltf' ? 'A self-contained JSON glTF with embedded geometry and textures. Preserves the saved asset; encoding usually makes it larger than GLB.'
      : savedAsset ? 'Exports the saved geometry, material and texture. Viewport display changes are excluded.' : 'This exports a procedural sample, not an AI reconstruction.'}</span></div>
    {savedAsset && format !== 'stl' && <p className="export-scale-note">For 1K or 2K textures, choose the color texture size in Refine and apply it before exporting. Export keeps the current texture resolution.</p>}
    <button className="primary-button full-width" disabled={busy || !ready || (format === 'stl' && !validHeight)}
      onClick={() => onExport(format, heightMm)}>
      {busy ? <LoaderCircle className="spin" size={16} /> : <Download size={16} />}
      {busy ? 'Saving asset…' : `Export ${format.toUpperCase()}`}<ArrowRight size={14} />
    </button>
  </>
}
