import { useEffect, useRef, useState } from 'react'
import { Brush, Eraser, LoaderCircle, RotateCcw, Undo2, X } from 'lucide-react'
import { prepareMask, saveMask, type MaskPreview } from '../harness'

interface Props {
  sourceId: string
  background: 'auto' | 'keep'
  initial: MaskPreview | null
  onSave: (mask: MaskPreview) => void
  onClose: () => void
}

export default function MaskEditor({ sourceId, background, initial, onSave, onClose }: Props) {
  const [preview, setPreview] = useState<MaskPreview | null>(null)
  const [busy, setBusy] = useState(true)
  const [message, setMessage] = useState('Preparing the foreground mask…')
  const [error, setError] = useState<string | null>(null)
  const [tool, setTool] = useState<'add' | 'erase'>('erase')
  const [size, setSize] = useState(24)
  const [view, setView] = useState<'overlay' | 'cutout' | 'mask'>('overlay')
  const [undoCount, setUndoCount] = useState(0)
  const canvas = useRef<HTMLCanvasElement>(null)
  const pixels = useRef<HTMLCanvasElement | null>(null)
  const photo = useRef<HTMLImageElement | null>(null)
  const original = useRef<ImageData | null>(null)
  const undo = useRef<ImageData[]>([])
  const last = useRef<{ x: number; y: number } | null>(null)
  const alive = useRef(true)

  useEffect(() => {
    alive.current = true
    const controller = new AbortController()
    void (initial ? Promise.resolve(initial) : prepareMask(sourceId, background, p => setMessage(p.message), controller.signal))
      .then(result => { if (!controller.signal.aborted) setPreview(result) })
      .catch(reason => { if (!controller.signal.aborted) { setError(String(reason)); setBusy(false) } })
    return () => { alive.current = false; controller.abort() }
  }, [sourceId, background, initial])

  const redraw = () => {
    const target = canvas.current, mask = pixels.current, image = photo.current
    if (!target || !mask || !image) return
    const ctx = target.getContext('2d')!
    ctx.clearRect(0, 0, target.width, target.height)
    if (view === 'mask') { ctx.drawImage(mask, 0, 0); return }
    ctx.drawImage(image, 0, 0)
    const data = ctx.getImageData(0, 0, target.width, target.height)
    const alpha = mask.getContext('2d')!.getImageData(0, 0, mask.width, mask.height).data
    for (let i = 0; i < data.data.length; i += 4) {
      const keep = alpha[i] / 255
      if (view === 'cutout') data.data[i + 3] = alpha[i]
      else {
        const opacity = (1 - keep) * 0.7
        data.data[i] = data.data[i] * (1 - opacity) + 117 * opacity
        data.data[i + 1] = data.data[i + 1] * (1 - opacity) + 49 * opacity
        data.data[i + 2] = data.data[i + 2] * (1 - opacity) + 87 * opacity
      }
    }
    ctx.putImageData(data, 0, 0)
  }

  useEffect(() => {
    if (!preview) return
    let cancelled = false
    const image = new Image(), mask = new Image()
    image.src = preview.imageDataUrl; mask.src = preview.maskDataUrl
    void Promise.all([image.decode(), mask.decode()]).then(() => {
      if (cancelled) return
      const buffer = document.createElement('canvas'); buffer.width = preview.width; buffer.height = preview.height
      buffer.getContext('2d', { willReadFrequently: true })!.drawImage(mask, 0, 0)
      photo.current = image; pixels.current = buffer
      original.current = buffer.getContext('2d')!.getImageData(0, 0, buffer.width, buffer.height)
      undo.current = []; setUndoCount(0); setBusy(false); redraw()
    }).catch(() => { if (!cancelled) { setError('Could not display this mask. Close and prepare it again.'); setBusy(false) } })
    return () => { cancelled = true }
    // Drawing reads the current view; switching the view does not reset edits.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [preview])
  useEffect(() => { redraw() }, [view])

  const snapshot = () => {
    const buffer = pixels.current!
    undo.current.push(buffer.getContext('2d')!.getImageData(0, 0, buffer.width, buffer.height))
    if (undo.current.length > 12) undo.current.shift()
    setUndoCount(undo.current.length)
  }
  const undoStroke = () => {
    const previous = undo.current.pop()
    if (previous && pixels.current) pixels.current.getContext('2d')!.putImageData(previous, 0, 0)
    setUndoCount(undo.current.length); redraw()
  }
  const paint = (event: React.PointerEvent<HTMLCanvasElement>, start: boolean) => {
    if (busy || !pixels.current || (start && event.button !== 0) || (!start && !last.current)) return
    event.preventDefault()
    const rect = event.currentTarget.getBoundingClientRect()
    const point = { x: (event.clientX - rect.left) * pixels.current.width / rect.width,
      y: (event.clientY - rect.top) * pixels.current.height / rect.height }
    if (start) { event.currentTarget.setPointerCapture(event.pointerId); snapshot(); last.current = point }
    const ctx = pixels.current.getContext('2d')!
    ctx.strokeStyle = tool === 'add' ? '#fff' : '#000'; ctx.fillStyle = ctx.strokeStyle
    ctx.lineWidth = size; ctx.lineCap = 'round'; ctx.lineJoin = 'round'
    ctx.beginPath(); ctx.moveTo(last.current!.x, last.current!.y); ctx.lineTo(point.x, point.y); ctx.stroke()
    ctx.beginPath(); ctx.arc(point.x, point.y, size / 2, 0, Math.PI * 2); ctx.fill()
    last.current = point; redraw()
  }
  const save = async () => {
    if (!pixels.current || busy) return
    setBusy(true); setError(null); setMessage('Saving this mask to your local library…')
    try {
      const result = await saveMask(sourceId, pixels.current.toDataURL('image/png'))
      if (alive.current) onSave(result)
    } catch (reason) { if (alive.current) { setError(String(reason)); setBusy(false) } }
  }

  return <div className="mask-backdrop"><section className="mask-editor" role="dialog" aria-modal="true" aria-labelledby="mask-title"
    onKeyDown={event => {
      if ((event.metaKey || event.ctrlKey) && event.key === 'z') { event.preventDefault(); if (!busy) undoStroke() }
      if (event.key === 'Tab') {
        const controls = Array.from(event.currentTarget.querySelectorAll<HTMLElement>('button:not(:disabled),input:not(:disabled)'))
        const first = controls[0], final = controls.at(-1)
        if (event.shiftKey && document.activeElement === first) { event.preventDefault(); final?.focus() }
        if (!event.shiftKey && document.activeElement === final) { event.preventDefault(); first?.focus() }
      }
    }}>
    <header><div><span className="section-label">SOURCE PREPARATION</span><h2 id="mask-title">Keep only your object.</h2><p>Erase the background or paint missing parts back in. Purple areas are excluded.</p></div><button autoFocus className="icon-button" aria-label="Close mask editor" onClick={onClose}><X size={20} /></button></header>
    <div className="mask-toolbar">
      <button disabled={busy} aria-pressed={tool === 'add'} onClick={() => setTool('add')}><Brush size={15} /> Add</button>
      <button disabled={busy} aria-pressed={tool === 'erase'} onClick={() => setTool('erase')}><Eraser size={15} /> Erase</button>
      <label>Brush <input aria-label="Brush size" type="range" min="2" max="100" value={size} onChange={e => setSize(Number(e.target.value))} />{size} px</label>
      <button disabled={busy || !undoCount} onClick={undoStroke}><Undo2 size={15} /> Undo</button>
      <button disabled={busy || !original.current} onClick={() => { snapshot(); pixels.current!.getContext('2d')!.putImageData(original.current!, 0, 0); redraw() }}><RotateCcw size={15} /> Reset</button>
    </div>
    <div className="mask-stage">{preview && <canvas ref={canvas} width={preview.width} height={preview.height} aria-label="Foreground mask canvas" onPointerDown={e => paint(e, true)} onPointerMove={e => paint(e, false)} onPointerUp={() => { last.current = null }} onPointerCancel={() => { last.current = null }} />}
      {busy && <div className="mask-progress" role="status"><LoaderCircle className="spin" size={22} /><span>{message}</span></div>}
      {!preview && error && <p>No mask prepared.</p>}
    </div>
    {error && <p role="alert" className="mask-error">{error}</p>}
    <footer><div className="mask-views">{(['overlay', 'cutout', 'mask'] as const).map(v => <button key={v} aria-pressed={view === v} onClick={() => setView(v)}>{v[0].toUpperCase() + v.slice(1)}</button>)}</div><p>Mask editing uses no trial generation.</p><button className="primary-button" disabled={busy || !pixels.current} onClick={() => void save()}>Use this mask</button></footer>
  </section></div>
}
