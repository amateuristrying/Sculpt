import { useEffect, useState } from 'react'
import { ArrowRight, LoaderCircle, SlidersHorizontal } from 'lucide-react'
import type { GeneratedAsset, RefinementSettings } from '../harness'

export default function RefinePanel({ asset, busy, onApply }: {
  asset: GeneratedAsset | null; busy: boolean; onApply: (settings: RefinementSettings) => void
}) {
  const defaults: RefinementSettings = { resolution: 128, densityThreshold: 25, removeSmallComponents: false, smoothingIterations: 0, textureResolution: 'vertex', targetFaceCount: null }
  const [settings, setSettings] = useState<RefinementSettings>(defaults)
  useEffect(() => {
    setSettings(asset?.metrics?.refinement ?? { ...defaults, resolution: asset?.metrics?.resolution ?? 128 })
  }, [asset?.id, asset?.metrics])
  const available = !!asset && !asset.simulated && !!asset.metrics?.canRefine
  return <section className="property-section refine-panel" aria-label="Refine geometry">
    <div className="section-label">REFINE GEOMETRY <SlidersHorizontal size={13} /></div>
    <p>{available ? 'Rebuild from the saved reconstruction. Each version is kept in your library.' : asset && !asset.simulated ? 'Generate this image again to save the reconstruction cache and enable refinement.' : 'Generate an image to adjust the underlying geometry.'}</p>
    <fieldset disabled={!available || busy}>
      <label className="field-label" htmlFor="extraction-resolution">Extraction resolution</label>
      <select id="extraction-resolution" value={settings.resolution} onChange={event => setSettings({ ...settings, resolution: Number(event.target.value) })}>
        {[96, 128, 192, 256].map(value => <option value={value} key={value}>{value}{value === 256 ? ' · detailed' : value === 96 ? ' · quick' : ''}</option>)}
      </select>
      <label className="field-label spaced" htmlFor="density-threshold">Surface density <output>{settings.densityThreshold}</output></label>
      <input id="density-threshold" type="range" min="10" max="40" step="1" value={settings.densityThreshold} onChange={event => setSettings({ ...settings, densityThreshold: Number(event.target.value) })} />
      <p>Lower values thicken surfaces; higher values trim them.</p>
      <label className="field-label spaced" htmlFor="smoothing-iterations">Smoothing iterations <output>{settings.smoothingIterations}</output></label>
      <input id="smoothing-iterations" type="range" min="0" max="10" step="1" value={settings.smoothingIterations} onChange={event => setSettings({ ...settings, smoothingIterations: Number(event.target.value) })} />
      <label className="refine-checkbox"><input type="checkbox" checked={settings.removeSmallComponents} onChange={event => setSettings({ ...settings, removeSmallComponents: event.target.checked })} />Remove tiny fragments</label>
      <p>Removes disconnected pieces below 0.5% of surface area. Small intentional details may be affected.</p>
      <label className="field-label spaced" htmlFor="texture-resolution">Texture output</label>
      <select id="texture-resolution" value={settings.textureResolution ?? 'vertex'} onChange={event => setSettings({ ...settings, textureResolution: event.target.value as RefinementSettings['textureResolution'], targetFaceCount: event.target.value === 'vertex' ? null : (settings.targetFaceCount ?? null) })}>
        <option value="vertex">Vertex colors · fastest</option>
        <option value="1k">Baked color · 1K</option>
        <option value="2k">Baked color · 2K</option>
      </select>
      {settings.textureResolution && settings.textureResolution !== 'vertex' && <>
        <label className="field-label spaced" htmlFor="target-face-count">Target faces</label>
        <select id="target-face-count" value={settings.targetFaceCount ?? ''} onChange={event => setSettings({ ...settings, targetFaceCount: event.target.value ? Number(event.target.value) : null })}>
          <option value="">Keep original geometry</option>
          {[5000, 10000, 20000, 50000].map(value => <option key={value} value={value}>{value.toLocaleString()}</option>)}
          {settings.targetFaceCount && ![5000, 10000, 20000, 50000].includes(settings.targetFaceCount) && <option value={settings.targetFaceCount}>{settings.targetFaceCount.toLocaleString()}</option>}
        </select>
        <p>Bakes color from the saved scene. Face targets are approximate; closed surfaces are preserved. Reducing faces may remove small details.</p>
      </>}
      <button className="primary-button full-width" onClick={() => onApply(settings)} disabled={!available || busy}>
        {busy ? <LoaderCircle size={14} className="spin" /> : <SlidersHorizontal size={14} />} Apply refinement <ArrowRight size={13} />
      </button>
    </fieldset>
    {available && <p className="refine-footnote">Uses your saved scene. No new image inference.</p>}
  </section>
}
