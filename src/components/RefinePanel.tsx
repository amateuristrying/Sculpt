import { useEffect, useState } from 'react'
import { ArrowRight, LoaderCircle, SlidersHorizontal } from 'lucide-react'
import type { GeneratedAsset, RefinementSettings } from '../harness'

export default function RefinePanel({ asset, busy, onApply }: {
  asset: GeneratedAsset | null; busy: boolean; onApply: (settings: RefinementSettings) => void
}) {
  const [settings, setSettings] = useState<RefinementSettings>({ resolution: 128, densityThreshold: 25, removeSmallComponents: false, smoothingIterations: 0 })
  useEffect(() => {
    setSettings(asset?.metrics?.refinement ?? { resolution: asset?.metrics?.resolution ?? 128, densityThreshold: 25, removeSmallComponents: false, smoothingIterations: 0 })
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
      <button className="primary-button full-width" onClick={() => onApply(settings)} disabled={!available || busy}>
        {busy ? <LoaderCircle size={14} className="spin" /> : <SlidersHorizontal size={14} />} Apply refinement <ArrowRight size={13} />
      </button>
    </fieldset>
    {available && <p className="refine-footnote">Uses your saved scene. No new image inference or trial charge.</p>}
  </section>
}
