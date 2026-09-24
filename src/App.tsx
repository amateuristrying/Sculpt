import { useCallback, useEffect, useRef, useState } from 'react';
import { ArrowDownToLine, ArrowLeft, ArrowRight, Box, Check, ChevronDown, ChevronRight, Circle, CircleDot, Cpu, Download, Expand, FileImage, Grid2X2, HelpCircle, Info, Layers3, LoaderCircle, LockKeyhole, Maximize, MousePointer2, Move, Plus, RotateCcw, RotateCw, Settings2, ShieldCheck, SlidersHorizontal, Sparkles, Sun, Upload, X } from 'lucide-react';
import { Brand, SculptMark } from './components/Brand';
import Setup from './components/Setup';
import RuntimeSetup from './components/RuntimeSetup';
import Library from './components/Library';
import RefinePanel from './components/RefinePanel';
import MaskEditor from './components/MaskEditor';
import Viewport, { type AssetMetadata, type ViewportApi, type ViewportMode } from './components/Viewport';
import { readMask, backendStatus, installRuntime, clearDownloadCache, importSource, readGeneratedAsset, readSource, listGenerationJobs, getAccessStatus, detectHardware, generate, refine, getEngines, saveGlb, saveGeneratedGlb } from './harness';
import { parseGeneratedGlb, type ImportedAsset } from './geometry/importedAsset';
import type { MaskPreview, RefinementSettings, AccessStatus, GenerationJob, SetupProgress, BackendStatus, EngineProfile, GeneratedAsset, GenerationProgress, HardwareProfile } from './harness/types';

type SourceImage = { name: string; url: string; width: number; height: number; size: number; sample: boolean; sourceId?: string };
type Quality = 'draft' | 'balanced' | 'high';
type PanelTab = 'stack' | 'inspector';
type Modal = 'export' | 'hardware' | 'help' | 'new' | 'library' | 'mask' | null;
const STAGES = ['Analyzing image', 'Loading local model', 'Generating geometry', 'Building surface', 'Preparing 3D asset'];
const MODE_OPTIONS: { id: ViewportMode; label: string; icon: typeof Box }[] = [{ id: 'material', label: 'Material', icon: Box }, { id: 'wireframe', label: 'Wireframe', icon: Expand }, { id: 'points', label: 'Points', icon: Grid2X2 }, { id: 'technical', label: 'Technical', icon: Layers3 }];
const COLORS = [{ name: 'Porcelain', value: '#ded8c9' }, { name: 'Graphite', value: '#646c69' }, { name: 'Sage', value: '#9fae91' }, { name: 'Clay', value: '#b88570' }];

export default function App() {
  const [hardware, setHardware] = useState<HardwareProfile | null>(null);
  const [engines, setEngines] = useState<EngineProfile[]>([]);
  const [access, setAccess] = useState<AccessStatus | null>(null);
  const [jobs, setJobs] = useState<GenerationJob[]>([]);
  const [libraryError, setLibraryError] = useState<string | null>(null);
  const [libraryBusy, setLibraryBusy] = useState(false);
  const [engineId, setEngineId] = useState('');
  const [setupError, setSetupError] = useState<string | null>(null);
  const [setupComplete, setSetupComplete] = useState(false);
  const [mask, setMask] = useState<MaskPreview | null>(null);
  const [source, setSource] = useState<SourceImage | null>(null);
  const [sourceLoading, setSourceLoading] = useState(false);
  const [dragging, setDragging] = useState(false);
  const [backend, setBackend] = useState<BackendStatus | null>(null);
  const [importedAsset, setImportedAsset] = useState<ImportedAsset | null>(null);
  const [installProgress, setInstallProgress] = useState<SetupProgress | null>(null);
  const [installing, setInstalling] = useState(false);
  const [clearingCache, setClearingCache] = useState(false);
  const [installError, setInstallError] = useState<string | null>(null);
  const installAbort = useRef<AbortController | null>(null);
  const [jobError, setJobError] = useState<string | null>(null);
  const [asset, setAsset] = useState<GeneratedAsset | null>(null);
  const [progress, setProgress] = useState<GenerationProgress | null>(null);
  const [generating, setGenerating] = useState(false);
  const [background, setBackground] = useState<'auto' | 'keep'>('auto');
  const [quality, setQuality] = useState<Quality>('balanced');
  const [mode, setMode] = useState<ViewportMode>('material');
  const [grid, setGrid] = useState(true);
  const [autoRotate, setAutoRotate] = useState(false);
  const [navigationMode, setNavigationMode] = useState<'orbit' | 'pan'>('orbit');
  const [lightIntensity, setLightIntensity] = useState(1);
  const [materialColor, setMaterialColor] = useState(COLORS[0].value);
  const [resetKey, setResetKey] = useState(0);
  const [panelTab, setPanelTab] = useState<PanelTab>('stack');
  const [metadata, setMetadata] = useState<AssetMetadata | null>(null);
  const [viewportReady, setViewportReady] = useState(false);
  const [modal, setModal] = useState<Modal>(null);
  const [exporting, setExporting] = useState(false);
  const [exportName, setExportName] = useState('Sculpt — Loop study');
  const [toast, setToast] = useState<string | null>(null);
  const [focusViewport, setFocusViewport] = useState(false);
  const fileInput = useRef<HTMLInputElement>(null);
  const viewportApi = useRef<ViewportApi | null>(null);
  const abortRef = useRef<AbortController | null>(null);
  const sourceRevision = useRef(0);
  const sourceLoadingRef = useRef(false);
  const selectedEngine = engines.find((engine) => engine.id === engineId);
  const reconstructionUnavailable = !!source && !source.sample && (hardware?.detectionSource !== 'native' || engineId === 'demo');
  const notify = useCallback((message: string) => setToast(message), []);
  const accessBlocked = hardware?.detectionSource === 'native' && engineId !== 'demo' && !access?.canGenerate;

  const refreshLibrary = useCallback(async () => {
    setLibraryBusy(true);
    try {
      const [history, status] = await Promise.all([listGenerationJobs(), getAccessStatus()]);
      setJobs(history); setAccess(status); setLibraryError(null);
    } catch (error) { setAccess(null); setLibraryError(error instanceof Error ? error.message : String(error)); }
    finally { setLibraryBusy(false); }
  }, []);
  useEffect(() => { void refreshLibrary(); }, [refreshLibrary]);

  const loadHardware = useCallback(async () => {
    setSetupError(null);
    try {
      const profile = await detectHardware();
      const [catalog, status] = await Promise.all([getEngines(profile), backendStatus()]);
      setBackend(status);
      setHardware(profile); setEngines(catalog);
      setEngineId((current) => current || catalog.find((engine) => engine.compatibility === 'recommended')?.id || catalog.find((engine) => engine.compatibility === 'available')?.id || '');
    } catch (error) { setSetupError(error instanceof Error ? error.message : 'Hardware detection could not complete.'); }
  }, []);
  useEffect(() => { void loadHardware(); }, [loadHardware]);
  useEffect(() => { if (!toast) return; const timeout = setTimeout(() => setToast(null), 5500); return () => clearTimeout(timeout); }, [toast]);
  useEffect(() => () => { abortRef.current?.abort(); installAbort.current?.abort(); }, []);

  const setupRuntime = async () => {
    if (installAbort.current || abortRef.current) return;
    const controller = new AbortController(); installAbort.current = controller;
    setInstalling(true); setInstallError(null); setInstallProgress(null);
    try { const status = await installRuntime(setInstallProgress, controller.signal); setBackend(status); }
    catch (error) { setInstallError(controller.signal.aborted ? 'Setup cancelled. You can retry and reuse downloaded files.' : error instanceof Error ? error.message : String(error)); }
    finally { installAbort.current = null; setInstalling(false); void loadHardware(); }
  };
  const runtimeSetup = { backend, progress: installProgress, busy: installing, error: installError, canInstall: !generating && !clearingCache && hardware?.detectionSource === 'native' && !!hardware?.isAppleSilicon && hardware.memoryGb >= 16, onInstall: () => void setupRuntime(), onCancel: () => installAbort.current?.abort(), onClearCache: () => {
    setClearingCache(true);
    void clearDownloadCache().then(setBackend).catch(error => notify(String(error))).finally(() => setClearingCache(false));
  } };

  const onViewportReady = useCallback((api: ViewportApi) => { viewportApi.current = api; setViewportReady(true); }, []);
  const onMetadata = useCallback((value: AssetMetadata) => setMetadata(value), []);

  const importImage = useCallback(async (file?: File) => {
    if (!file || abortRef.current) return;
    if (!['image/png', 'image/jpeg', 'image/webp'].includes(file.type)) { notify('Choose a PNG, JPG, or WEBP image.'); return; }
    if (file.size > 30 * 1024 * 1024) { notify('Please choose an image smaller than 30 MB.'); return; }
    const revision = ++sourceRevision.current;
    sourceLoadingRef.current = true; setSourceLoading(true);
    try {
      const url = await new Promise<string>((resolve, reject) => { const reader = new FileReader(); reader.onload = () => resolve(String(reader.result)); reader.onerror = () => reject(new Error('Could not read this image.')); reader.readAsDataURL(file); });
      const nativeSource = await importSource(file.name, url);
      const image = new Image(); image.src = url; await image.decode();
      if (revision !== sourceRevision.current) return;
      const reconstructionEngine = engines.find(engine => engine.id !== 'demo' && engine.implemented && engine.compatibility !== 'unsupported');
      if (nativeSource && reconstructionEngine) setEngineId(reconstructionEngine.id);
      setSource({ sourceId: nativeSource?.id, name: file.name, url, width: image.naturalWidth, height: image.naturalHeight, size: file.size, sample: false });
      setMask(null); setAsset(null); setImportedAsset(null); setJobError(null); setProgress(null); setExportName(file.name.replace(/\.[^.]+$/, ''));
    } catch (error) { if (revision === sourceRevision.current) notify(error instanceof Error ? error.message : String(error)); }
    finally { if (revision === sourceRevision.current) { sourceLoadingRef.current = false; setSourceLoading(false); } }
  }, [engines, notify]);

  const useSample = async () => {
    if (!viewportApi.current || abortRef.current || sourceLoadingRef.current) return;
    const revision = ++sourceRevision.current;
    sourceLoadingRef.current = true; setSourceLoading(true);
    try {
      setMask(null); setEngineId('demo');
      const url = viewportApi.current.capturePreview();
      const image = new Image(); image.src = url; await image.decode();
      if (revision !== sourceRevision.current) return;
      setSource({ name: 'Loop study.png', url, width: image.naturalWidth, height: image.naturalHeight, size: Math.round(url.length * 0.75), sample: true }); setAsset(null); setImportedAsset(null); setJobError(null); setProgress(null); setExportName('Sculpt — Loop study');
    } catch { if (revision === sourceRevision.current) notify('The preview is still preparing. Please try again.'); }
    finally { if (revision === sourceRevision.current) { sourceLoadingRef.current = false; setSourceLoading(false); } }
  };

  const startGeneration = useCallback(async () => {
    if (!source || reconstructionUnavailable || accessBlocked || abortRef.current || installAbort.current || sourceLoadingRef.current || !engineId) return;
    if (source.sourceId && !mask && background === 'auto') { setModal('mask'); return; }
    const revision = sourceRevision.current;
    const controller = new AbortController(); abortRef.current = controller;
    setGenerating(true); setJobError(null); setProgress({ jobId: '', stage: 'analyzing', progress: 0, message: STAGES[0] });
    try {
      const result = await generate({ engineId, imageName: source.name, sourceId: source.sourceId, geometry: quality, background, maskSha256: mask?.sha256 }, setProgress, controller.signal);
      if (controller.signal.aborted || revision !== sourceRevision.current) return;
      const model = result.simulated ? null : await parseGeneratedGlb(await readGeneratedAsset(result.id));
      if (controller.signal.aborted || revision !== sourceRevision.current) { model?.dispose(); return; }
      setImportedAsset(model); setAsset(result); setResetKey((key) => key + 1); setPanelTab('stack'); notify(result.simulated ? 'Demo sculpture ready. This mode does not reconstruct images.' : `Image reconstructed locally${result.metrics ? ` in ${Math.round(result.metrics.totalSeconds)} seconds` : ''}.`);
    } catch (error) {
      if (controller.signal.aborted || (error instanceof Error && error.name === 'AbortError')) { setProgress(null); notify('Generation cancelled. Your source image is ready when you are.'); }
      else { setProgress(null); setJobError(error instanceof Error ? error.message : String(error)); }
    } finally { setGenerating(false); abortRef.current = null; void refreshLibrary(); }
  }, [source, mask, reconstructionUnavailable, accessBlocked, engineId, quality, background, notify, refreshLibrary]);

  const openJob = async (job: GenerationJob) => {
    if (abortRef.current || sourceLoadingRef.current) return;
    const revision = ++sourceRevision.current;
    sourceLoadingRef.current = true; setSourceLoading(true);
    let model: ImportedAsset | null = null;
    try {
      let sourceError: unknown;
      const saved = job.request.sourceId ? await readSource(job.request.sourceId).catch(error => { sourceError = error; return null; }) : null;
      if (sourceError && !job.asset) throw sourceError;
      if (job.asset && !job.asset.simulated) model = await parseGeneratedGlb(await readGeneratedAsset(job.id));
      let restoredMask: MaskPreview | null = null;
      let maskError: unknown;
      if (saved && job.request.maskSha256) restoredMask = await readMask(saved.asset.id, job.request.maskSha256).catch(error => { maskError = error; return null; });
      if (revision !== sourceRevision.current) { model?.dispose(); return; }
      setMask(restoredMask);
      setSource(saved ? { sourceId: saved.asset.id, name: saved.asset.name, url: saved.dataUrl, width: saved.asset.width, height: saved.asset.height, size: saved.size, sample: false } : null);
      setAsset(job.asset ?? null); setImportedAsset(model); setEngineId(job.request.engineId);
      setQuality(job.request.geometry); setBackground(job.request.background ?? 'auto');
      setExportName(job.request.imageName.replace(/\.[^.]+$/, '')); setJobError(null); setProgress(null);
      setResetKey(key => key + 1); setModal(null);
      notify(maskError ? 'Asset reopened. The saved mask is unavailable; review the foreground before generating again.' : sourceError ? 'Asset reopened. The original source image is missing or damaged; export is still available.' : job.asset ? 'Saved asset reopened from your local library.' : 'Source restored. You can retry generation.');
    } catch (error) { model?.dispose(); if (revision === sourceRevision.current) setLibraryError(error instanceof Error ? error.message : String(error)); }
    finally { if (revision === sourceRevision.current) { sourceLoadingRef.current = false; setSourceLoading(false); } }
  };

  const applyRefinement = async (settings: RefinementSettings) => {
    if (!asset || asset.simulated || abortRef.current || installAbort.current || sourceLoadingRef.current) return;
    const revision = sourceRevision.current;
    const controller = new AbortController(); abortRef.current = controller;
    setGenerating(true); setJobError(null); setProgress({ jobId: '', stage: 'surface', progress: 0, message: 'Opening the saved reconstruction' });
    try {
      const result = await refine(asset.id, settings, setProgress, controller.signal);
      if (controller.signal.aborted || revision !== sourceRevision.current) return;
      const model = await parseGeneratedGlb(await readGeneratedAsset(result.id));
      if (controller.signal.aborted || revision !== sourceRevision.current) { model.dispose(); return; }
      setImportedAsset(model); setAsset(result); setResetKey(key => key + 1);
      notify('Refined version saved. The original is still in your local library.');
    } catch (error) {
      setProgress(null);
      if (controller.signal.aborted) notify('Refinement cancelled. Your saved asset is unchanged.');
      else setJobError(error instanceof Error ? error.message : String(error));
    } finally { setGenerating(false); abortRef.current = null; void refreshLibrary(); }
  };

  const clearSource = () => { ++sourceRevision.current; sourceLoadingRef.current = false; setSourceLoading(false); setSource(null); setMask(null); setAsset(null); setImportedAsset(null); setJobError(null); setProgress(null); };
  const resetProject = () => { clearSource(); setMode('material'); setNavigationMode('orbit'); setQuality('balanced'); setBackground('auto'); setMaterialColor(COLORS[0].value); setAutoRotate(false); setGrid(true); setLightIntensity(1); setExportName('Sculpt — Loop study'); setResetKey((key) => key + 1); setModal(null); };
  const exportAsset = async () => {
    if (!viewportApi.current || exporting) return;
    setExporting(true);
    try {
      const name = exportName.trim().replace(/[<>:"/\\|?*\u0000-\u001f]/g, '-').replace(/\.glb$/i, '') || 'Sculpt asset';
      const savedPath = asset && !asset.simulated
        ? await saveGeneratedGlb(asset.id, `${name}.glb`)
        : await saveGlb(await viewportApi.current.exportGlb(), `${name}.glb`);
      if (savedPath) { setModal(null); notify(`GLB exported · ${savedPath}`); }
    } catch (error) { notify(error instanceof Error ? error.message : 'Could not export the asset. Please try again.'); }
    finally { setExporting(false); }
  };

  useEffect(() => {
    const keydown = (event: KeyboardEvent) => {
      const inputFocused = event.target instanceof HTMLInputElement || event.target instanceof HTMLTextAreaElement || event.target instanceof HTMLSelectElement;
      if (event.key === 'Escape') { if (!exporting) setModal(null); setFocusViewport(false); return; }
      if (inputFocused || !setupComplete || modal) return;
      if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === 'o') { event.preventDefault(); if (!generating) fileInput.current?.click(); }
      if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === 'e') { event.preventDefault(); if (viewportReady && !generating) setModal('export'); }
      if (!event.metaKey && !event.ctrlKey) {
        if (event.key.toLowerCase() === 'f') setResetKey((key) => key + 1);
        if (event.key.toLowerCase() === 'g') setGrid((value) => !value);
        if (['1', '2', '3', '4'].includes(event.key)) setMode(MODE_OPTIONS[Number(event.key) - 1].id);
      }
    };
    window.addEventListener('keydown', keydown); return () => window.removeEventListener('keydown', keydown);
  }, [setupComplete, modal, exporting, generating, viewportReady]);

  const stageIndex = progress ? ['analyzing', 'loading', 'geometry', 'surface', 'preparing', 'complete'].indexOf(progress.stage) : 0;
  return <div className="app-shell">
    {modal === 'mask' && source?.sourceId && <MaskEditor sourceId={source.sourceId} background={background} initial={mask} onClose={() => setModal(null)} onSave={value => { setMask(value); setModal(null); notify('Foreground mask saved. Generate to reconstruct this selection.'); }} />}
    <div className="titlebar" data-tauri-drag-region><div className="titlebar-leading" data-tauri-drag-region><span className="titlebar-app">Sculpt</span><span className="titlebar-divider" /> <span>Local 3D studio</span></div><span className="titlebar-project" data-tauri-drag-region>{setupComplete ? source?.name.replace(/\.[^.]+$/, '') || 'Untitled project' : 'Welcome to Sculpt'}</span><span className="titlebar-status"><span className="live-dot" /> On your device</span></div>
    {!setupComplete ? <Setup runtimeSetup={runtimeSetup} backend={backend} hardware={hardware} engines={engines} selectedEngine={engineId} onSelect={setEngineId} onContinue={() => setSetupComplete(true)} error={setupError} onRetry={() => void loadHardware()} /> : <>
      <header className="workspace-header"><Brand compact /><nav className="workflow-nav" aria-label="Asset workflow"><button className={!asset ? 'current' : 'completed'} onClick={() => fileInput.current?.click()} disabled={generating}><span>01</span> Image</button><ChevronRight size={12} /><button className={asset ? 'current' : ''} onClick={() => { setPanelTab('stack'); setFocusViewport(false); }}><span>02</span> Geometry</button><ChevronRight size={12} /><button onClick={() => { setPanelTab('stack'); setFocusViewport(false); document.querySelector('.refine-panel')?.scrollIntoView({ block: 'nearest' }); }}><span>03</span> Refine</button><ChevronRight size={12} /><button onClick={() => setModal('export')} disabled={!viewportReady || generating}><span>04</span> Export</button></nav><div className="header-actions"><button className="hardware-button" onClick={() => setModal('hardware')}><Cpu size={14} />{hardware?.chip}<span className="live-dot" /></button><button className="export-button" onClick={() => setModal('export')} disabled={!viewportReady || generating}><ArrowDownToLine size={14} /> Export <ChevronDown size={12} /></button></div></header>
      <div className={`workspace ${focusViewport ? 'viewport-focused' : ''}`}>
        <aside className="source-panel"><div className="source-controls">
          <div className="panel-title"><span>PROJECT</span><button className="icon-button" title="New project" aria-label="New project" disabled={generating} onClick={() => source ? setModal('new') : resetProject()}><Plus size={16} /></button></div>
          <div className="project-name"><span className="project-icon"><Box size={18} /></span><div><h2>{source ? source.name.replace(/\.[^.]+$/, '') : 'Untitled project'}</h2><p>{asset ? '1 generated asset' : 'A new dimension awaits'}</p></div></div>
          <button className="library-button secondary-button" disabled={generating || sourceLoading} onClick={() => { setModal('library'); void refreshLibrary(); }}>Local library <span>{jobs.filter(job => job.state === 'succeeded').length}</span><ChevronRight size={13} /></button>
          <div className="source-section"><div className="section-label">SOURCE IMAGE <span>01</span></div>
            <input ref={fileInput} type="file" accept="image/png,image/jpeg,image/webp" hidden onChange={(event) => { void importImage(event.target.files?.[0]); event.target.value = ''; }} />
            <div className={`drop-zone ${source ? 'has-image' : ''} ${dragging ? 'dragging' : ''}`} onDragOver={(event) => { event.preventDefault(); if (!generating) setDragging(true); }} onDragLeave={() => setDragging(false)} onDrop={(event) => { event.preventDefault(); setDragging(false); void importImage(event.dataTransfer.files[0]); }}>
              {source ? <><img src={source.url} alt={`Source image: ${source.name}`} /><button className="replace-image" onClick={() => fileInput.current?.click()} disabled={generating}><Upload size={13} /> Replace image</button><span className="image-tag">{source.sample ? 'SAMPLE' : 'SOURCE'}</span></> : <button className="drop-zone-button" onClick={() => fileInput.current?.click()}><span className="upload-illustration"><FileImage size={27} strokeWidth={1.2} /><span><Plus size={12} /></span></span><strong>Drop an image here</strong><span>or click to browse</span><small>PNG / JPG / WEBP</small></button>}
            </div>
            {source ? <div className="source-file"><FileImage size={13} /><span>{source.name}</span><button className="icon-button" aria-label="Remove source image" disabled={generating} onClick={clearSource}><X size={12} /></button><small>{source.width} × {source.height} <span>·</span> {Math.max(1, Math.round(source.size / 1024)).toLocaleString()} KB</small></div> : <button className="sample-button" onClick={() => void useSample()} disabled={!viewportReady || sourceLoading}><Sparkles size={13} /> Explore the demo <ArrowRight size={12} /></button>}
          </div>
          <div className="generation-settings"><div className="section-label">GENERATION <Settings2 size={13} /></div><label className="field-label">Geometry quality <Info size={12}><title>Controls extraction resolution. Higher quality uses more memory and takes longer.</title></Info></label><Segments options={['Draft', 'Balanced', 'High']} value={quality[0].toUpperCase() + quality.slice(1)} disabled={generating} onChange={(value) => setQuality(value.toLowerCase() as Quality)} /><div className="quality-caption">{quality === 'draft' ? 'Quick studies. A lighter mesh.' : quality === 'high' ? 'More detail. A denser mesh.' : 'A considered balance of detail and speed.'}</div><p className="quality-caption">{engineId !== 'demo' && backend?.qualities?.find(p => p.id === quality) && `Estimated working memory: ${backend.qualities.find(p => p.id === quality)!.estimatedMemoryGb} GB. ${quality === 'high' && (hardware?.memoryGb || 0) < 24 ? 'High is best on Macs with 24 GB or more.' : 'Balanced is recommended on a 16 GB Mac.'}`}</p><label className="field-label spaced">Background</label><Segments options={['Auto remove', 'Keep']} value={background === 'auto' ? 'Auto remove' : 'Keep'} disabled={generating} onChange={value => { setBackground(value === 'Keep' ? 'keep' : 'auto'); setMask(null); }} /><p className="quality-caption">{background === 'keep' ? 'Keep preserves the background. Use a tightly cropped object on a plain background.' : 'Automatic masking works best with one clear, unobstructed object.'}</p>{source?.sourceId && !source.sample && <button className="mask-review-button" disabled={generating || !backend?.installed || sourceLoading} onClick={() => setModal('mask')}>{mask ? 'Edit foreground mask' : 'Review foreground'}<span>{mask ? '✓ Saved' : '→'}</span></button>}<label className="field-label engine-field">Engine profile <span className="tiny-pill">{engineId === 'demo' ? 'DEMO' : 'LOCAL AI'}</span></label><button className="selected-engine" onClick={() => setModal('hardware')} disabled={generating}><span className="engine-mini-icon"><Cpu size={15} /></span><span><strong>{selectedEngine?.name || 'Local simulator'}</strong><small>{selectedEngine?.subtitle || 'Sculpt Inference Harness'}</small></span><ChevronDown size={13} /></button>
          </div>
          </div><div className="generation-bottom">{accessBlocked && <div className="generation-error" role="status"><p>{libraryError || access?.message || 'Checking local access…'}</p><button className="text-button" onClick={() => { setModal('library'); void refreshLibrary(); }}>Open library &amp; license</button></div>}{reconstructionUnavailable && <div role="status" className="generation-error"><strong>{hardware?.detectionSource === 'preview' ? 'Desktop app required' : 'Select a reconstruction engine'}</strong><p>{hardware?.detectionSource === 'preview' ? 'This browser preview cannot reconstruct your image. Open the Sculpt macOS app to generate with your local AI engine.' : 'Workspace Demo does not use your photo. Choose a supported local AI engine in the engine settings.'}</p></div>}{jobError && <div role="alert" className="generation-error"><strong>Reconstruction stopped</strong><p>{jobError}</p><button className="text-button" onClick={() => setJobError(null)}>Dismiss</button></div>}{engineId !== 'demo' && !backend?.installed && <div className="generation-error"><p>{backend?.message || 'Checking runtime…'}</p><button className="text-button" onClick={() => void loadHardware()}>Check again</button></div>}{generating ? <button className="primary-button generating-button" onClick={() => abortRef.current?.abort()}><LoaderCircle size={15} className="spin" /> Cancel generation <X size={13} /></button> : <button className="primary-button generate-button" disabled={!source || reconstructionUnavailable || accessBlocked || !viewportReady || sourceLoading || installing || (engineId !== 'demo' && !backend?.installed)} onClick={() => void startGeneration()}><SculptMark size={16} />{sourceLoading ? 'Opening image…' : reconstructionUnavailable ? 'Reconstruction unavailable' : engineId === 'demo' ? 'Generate demo' : source?.sourceId && !mask && background === 'auto' ? 'Review foreground' : asset ? 'Generate again' : 'Generate 3D'}<ArrowRight size={14} /></button>}<p><LockKeyhole size={11} /> Your image stays on this device.</p></div>
          <div className="local-note"><ShieldCheck size={16} /><div><strong>A studio without a server.</strong><span>Built for your hardware.<br />Designed to stay yours.</span></div></div>
          <button className="help-button" onClick={() => setModal('help')}><HelpCircle size={14} /> A little guidance <span>?</span></button>
        </aside>
        <main className="viewport-panel">
          <div className="viewport-topbar"><div className="viewport-title"><Box size={14} /><span>{asset ? 'Generated asset' : 'Loop study'}</span><span className="viewport-title-tag">{asset ? asset.simulated ? 'DEMO' : 'RECONSTRUCTED' : 'SAMPLE ASSET'}</span></div><div className="viewport-top-actions"><span>Perspective</span><button className={`icon-button ${focusViewport ? 'active' : ''}`} title="Focus viewport" aria-label="Focus viewport" onClick={() => setFocusViewport(!focusViewport)}><Maximize size={14} /></button></div></div>
          <div className="viewport-stage"><Viewport importedAsset={importedAsset} mode={mode} navigationMode={navigationMode} grid={grid || mode === 'technical'} autoRotate={autoRotate} lightIntensity={lightIntensity} materialColor={materialColor} resetKey={resetKey} assetSeed={asset?.seed ?? 0} geometryQuality={quality} onMetadata={onMetadata} onReady={onViewportReady} />
            <div className="viewport-mode-label"><span className="live-dot" />{MODE_OPTIONS.find((option) => option.id === mode)?.label.toUpperCase()} VIEW<span>{mode === 'points' ? 'VERTEX CLOUD' : mode === 'technical' ? 'SURFACE / EDGES / VERTICES' : 'LOCAL WORKSPACE'}</span></div>
            <div className="viewport-side-tools"><ToolButton label="Orbit tool" active={navigationMode === 'orbit'} icon={MousePointer2} onClick={() => setNavigationMode('orbit')} /><ToolButton label="Pan tool" active={navigationMode === 'pan'} icon={Move} onClick={() => setNavigationMode('pan')} /><span /><ToolButton label="Reset camera (F)" icon={RotateCcw} onClick={() => setResetKey((key) => key + 1)} /><ToolButton label="Toggle grid (G)" icon={Grid2X2} active={grid || mode === 'technical'} onClick={() => { if (mode === 'technical') notify('Technical mode always shows its reference grid. Switch views to hide it.'); else setGrid(!grid); }} /><ToolButton label="Auto-rotate" icon={RotateCw} active={autoRotate} onClick={() => setAutoRotate(!autoRotate)} /></div>
            {!asset && !generating && <div className="sample-hint"><span className="hint-line" /><p>A study in possibility.<span>{reconstructionUnavailable ? 'Open Sculpt desktop to reconstruct this image.' : source ? 'Your source is ready. Generate to explore the workflow.' : 'Start with an image. Make it something more.'}</span></p></div>}
            {generating && <div className="generation-overlay" role="status" aria-live="polite"><div className="generation-orbit"><SculptMark size={29} /></div><span className="section-kicker">LOCAL GENERATION · {engineId === 'demo' ? 'DEMO' : 'ON DEVICE'}</span><h2>{progress?.message || 'Preparing your asset'}</h2><div className="progress-track"><span style={{ width: `${progress?.progress || 0}%` }} /></div><div className="progress-stages">{STAGES.map((stage, index) => <div key={stage} className={index < stageIndex ? 'done' : index === stageIndex ? 'current' : ''}>{index < stageIndex ? <Check size={12} /> : index === stageIndex ? <LoaderCircle className="spin" size={12} /> : <Circle size={10} />}<span>{stage}</span></div>)}</div><button className="text-button" onClick={() => abortRef.current?.abort()}>Cancel</button></div>}
            <div className="viewport-bottom-controls"><div className="view-switcher" role="group" aria-label="Visualization mode">{MODE_OPTIONS.map(({ id, label, icon: Icon }, index) => <button title={`${label} view (${index + 1})`} key={id} aria-pressed={mode === id} className={mode === id ? 'selected' : ''} onClick={() => setMode(id)}><Icon size={13} /><span>{label}</span></button>)}</div></div>
          </div>
          <footer className="viewport-footer"><span><MousePointer2 size={11} /> {navigationMode === 'pan' ? 'Drag to pan' : 'Drag to orbit'} <span>·</span> Shift + drag to pan <span>·</span> Scroll to zoom</span><span><span className="live-dot" />{generating ? `${Math.round(progress?.progress || 0)}%` : 'Ready'}</span></footer>
        </main>
        <aside className="properties-panel"><div className="properties-tabs"><button className={panelTab === 'stack' ? 'active' : ''} onClick={() => setPanelTab('stack')}><Layers3 size={13} /> Sculpt stack</button><button className={panelTab === 'inspector' ? 'active' : ''} onClick={() => setPanelTab('inspector')}><SlidersHorizontal size={13} /> Inspector</button></div>
          <div className="properties-scroll">{panelTab === 'stack' ? <>
            <div className="stack-intro"><h3>From image to asset.</h3><p>Every step, in your hands.</p></div>
            <div className="stack-list"><StackRow number="01" icon={FileImage} label="Image" subtitle={source ? 'Source ready' : 'Waiting for source'} status={source ? 'complete' : 'idle'} /><StackRow number="02" icon={Box} label="Geometry" subtitle={generating ? 'Generating…' : asset ? 'Geometry generated' : 'Ready to generate'} status={generating ? 'running' : asset ? 'complete' : 'idle'} active /><StackRow number="03" icon={Sparkles} label="Mesh refinement" subtitle={asset?.metrics?.operation === 'refine' ? 'Refined version saved' : 'Resolution, density & cleanup'} status={asset?.metrics?.operation === 'refine' ? 'complete' : 'idle'} /><StackRow number="07" icon={CircleDot} label="Material" subtitle={importedAsset ? 'Reconstructed vertex colors' : `${COLORS.find((color) => color.value === materialColor)?.name || 'Custom'} · preview`} status="preview" /><div className="stack-final"><Box size={15} /><span>Final asset</span><span className="tiny-pill">{asset ? asset.simulated ? 'DEMO READY' : 'ASSET READY' : 'PREVIEW'}</span></div></div><div className="stack-disclosure"><Info size={12} /><p>Refine rebuilds saved geometry. UV mapping, retopology and texture baking are not available yet.</p></div>
            <RefinePanel asset={asset} busy={generating || installing || sourceLoading} onApply={settings => void applyRefinement(settings)} />
          </> : <><div className="stack-intro"><h3>Look a little closer.</h3><p>The structure behind the surface.</p></div><div className="property-section inspector-section"><div className="section-label">DISPLAY</div><label className="field-label" htmlFor="visualization">Visualization</label><select id="visualization" value={mode} onChange={(event) => setMode(event.target.value as ViewportMode)}>{MODE_OPTIONS.map((option) => <option value={option.id} key={option.id}>{option.label} View</option>)}</select><div className="toggle-row"><span>Perspective grid</span><Toggle label="Perspective grid" enabled={grid || mode === 'technical'} disabled={mode === 'technical'} onToggle={() => setGrid(!grid)} /></div><div className="toggle-row"><span>Turntable rotation</span><Toggle label="Turntable rotation" enabled={autoRotate} onToggle={() => setAutoRotate(!autoRotate)} /></div><button className="secondary-button" onClick={() => setResetKey((key) => key + 1)}><RotateCcw size={13} /> Reset camera <kbd>F</kbd></button></div></>}
          <div className="property-section material-section"><div className="section-label">MATERIAL PREVIEW <CircleDot size={12} /></div><div className="swatch-row">{COLORS.map((color) => <button disabled={!!importedAsset} key={color.value} aria-label={color.name} title={color.name} aria-pressed={materialColor === color.value} className={`material-swatch ${materialColor === color.value ? 'selected' : ''}`} style={{ '--swatch': color.value } as React.CSSProperties} onClick={() => setMaterialColor(color.value)}>{materialColor === color.value && <Check size={12} />}</button>)}<span>{importedAsset ? 'Source colors' : COLORS.find((color) => color.value === materialColor)?.name}</span></div><div className="light-label"><span><Sun size={13} /> Studio lighting</span><span>{Math.round(lightIntensity * 100)}%</span></div><input aria-label="Studio lighting" className="light-slider" type="range" min="0.3" max="2" step="0.05" value={lightIntensity} onChange={(event) => setLightIntensity(Number(event.target.value))} /></div>
          <div className="property-section asset-metadata"><div className="section-label">ASSET DETAILS <span className="tiny-pill">{asset ? asset.simulated ? 'DEMO' : 'RECONSTRUCTED' : 'SAMPLE'}</span></div><dl><div><dt>Faces</dt><dd>{metadata?.faces.toLocaleString() || '—'}</dd></div><div><dt>Vertices</dt><dd>{metadata?.vertices.toLocaleString() || '—'}</dd></div><div><dt>Materials</dt><dd>{metadata?.materials || '—'}</dd></div><div><dt>Texture resolution</dt><dd>{metadata?.textureResolution || 'None'}</dd></div><div><dt>Format</dt><dd>{metadata?.format || 'GLB'}</dd></div>{asset?.metrics && <><div><dt>Compute</dt><dd>{asset.metrics.device === 'mps' ? 'Metal GPU' : asset.metrics.device}</dd></div><div><dt>Generation</dt><dd>{Math.round(asset.metrics.totalSeconds)} s</dd></div>{asset.metrics.meshQuality && <><div><dt>Closed surface</dt><dd>{asset.metrics.meshQuality.watertight ? 'Yes' : 'Open edges'}</dd></div><div><dt>Mesh parts</dt><dd>{asset.metrics.meshQuality.components}</dd></div></>}</>}</dl></div></div>
        </aside>
      </div><footer className="app-statusbar"><span><SculptMark size={11} /> SCULPT <span className="status-separator">/</span> 0.1 PREVIEW</span><span><span className="live-dot" /> {hardware?.chip} <span>·</span> {hardware?.memoryGb} GB {hardware?.unifiedMemory ? 'unified memory' : 'RAM'} <span>·</span> {hardware?.detectionSource === 'preview' ? 'Preview profile' : 'Hardware detected'}</span><span><LockKeyhole size={10} /> {engineId === 'demo' ? 'Workspace demo' : backend?.installed ? 'Local AI ready' : 'Runtime setup required'} <span>·</span> {generating ? 'Processing on device' : 'Idle'}</span></footer>
    </>}
    {modal && modal !== 'mask' && <Dialog title={modal === 'library' ? 'Your local library.' : modal === 'export' ? 'Ready for the next step.' : modal === 'hardware' ? 'Your local environment.' : modal === 'new' ? 'Start a new project?' : 'Find your way around.'} onClose={() => { if (!exporting) setModal(null); }}>
      {modal === 'library' ? <Library jobs={jobs} access={access} error={libraryError} busy={libraryBusy || sourceLoading} onRefresh={() => void refreshLibrary()} onOpen={job => void openJob(job)} onActivated={setAccess} /> : modal === 'export' ? <><span className="dialog-kicker">EXPORT ASSET</span><p className="dialog-description">Take your geometry wherever you create next.</p><div className="export-format-options"><button className="selected"><Box size={20} /><strong>GLB</strong><small>Geometry + material</small><Check size={13} /></button><button disabled><Box size={20} /><strong>OBJ</strong><small>Coming later</small></button><button disabled><Box size={20} /><strong>STL</strong><small>Coming later</small></button></div><label className="field-label" htmlFor="export-name">File name</label><div className="export-name"><input id="export-name" value={exportName} onChange={(event) => setExportName(event.target.value)} /><span>.glb</span></div><div className="export-summary"><span>{metadata?.faces.toLocaleString()} faces</span><span>1 material</span><span>{importedAsset ? 'Vertex colors' : 'No textures'}</span></div><div className="dialog-note"><Info size={14} /><span>{asset ? asset.simulated ? 'This is a procedural demo asset.' : 'Reconstructed from your source image on this device. Export preserves the original geometry and vertex colors.' : 'You’re exporting the built-in sample sculpture.'}</span></div><button className="primary-button full-width" onClick={() => void exportAsset()} disabled={exporting || !viewportReady}>{exporting ? <LoaderCircle className="spin" size={16} /> : <Download size={16} />}{exporting ? 'Saving asset…' : 'Export GLB'}<ArrowRight size={14} /></button></> : modal === 'hardware' ? <><span className="dialog-kicker">SCULPT INFERENCE HARNESS</span><p className="dialog-description">The hardware is yours. The possibilities follow.</p><div className="hardware-modal-summary"><Cpu size={26} /><div><strong>{hardware?.chip}</strong><span>{hardware?.memoryGb} GB {hardware?.unifiedMemory ? 'Unified Memory' : 'RAM'} · {hardware?.architecture}</span></div><span className="live-dot" /></div><dl className="hardware-modal-details"><div><dt>Operating system</dt><dd>{hardware?.os} {hardware?.osVersion}</dd></div><div><dt>Graphics</dt><dd>{hardware?.gpu}</dd></div><div><dt>Compute capability</dt><dd>{hardware?.computeBackends.join(', ') || 'Unknown'}</dd></div><div><dt>Detection</dt><dd>{hardware?.detectionSource === 'native' ? 'Native hardware APIs' : 'M4 development fixture'}</dd></div></dl><label className="field-label" htmlFor="engine-profile">Engine profile</label><select id="engine-profile" value={engineId} disabled={generating} onChange={(event) => setEngineId(event.target.value)}>{engines.map((engine) => <option key={engine.id} value={engine.id} disabled={engine.compatibility === 'unsupported'}>{engine.name} · {engine.compatibility === 'unsupported' ? 'Unsupported' : engine.implemented ? 'Available' : 'Planned'}</option>)}</select><div className="dialog-note"><Info size={14} /><span>{selectedEngine?.reason} {engineId !== 'demo' && backend?.message}</span></div><RuntimeSetup {...runtimeSetup} /><div className="harness-path"><span>UI</span><ChevronRight size={12} /><strong>Sculpt Harness</strong><ChevronRight size={12} /><span>Runtime</span><ChevronRight size={12} /><span>Engine</span></div><button className="secondary-button full-width" disabled={generating} onClick={() => { setModal(null); setSetupComplete(false); }}><ArrowLeft size={13} /> Return to setup</button></> : modal === 'new' ? <><p className="dialog-description">Start a fresh workspace. Desktop generations remain saved in your local library.</p><div className="dialog-button-row"><button className="secondary-button" onClick={() => setModal('export')}><Download size={14} /> Export first</button><button className="primary-button" onClick={resetProject}>New project <Plus size={14} /></button></div></> : <><span className="dialog-kicker">A LITTLE GUIDANCE</span><p className="dialog-description">Drop in an image, reconstruct your object, then explore the geometry beneath the surface.</p><div className="shortcut-list">{[['Import image', '⌘ O'], ['Export GLB', '⌘ E'], ['Frame / reset camera', 'F'], ['Toggle grid', 'G'], ['Material / Wireframe / Points / Technical', '1 – 4'], ['Orbit', 'Drag'], ['Pan', 'Shift + drag'], ['Zoom', 'Scroll']].map(([label, shortcut]) => <div key={label}><span>{label}</span><kbd>{shortcut}</kbd></div>)}</div><div className="dialog-note"><ShieldCheck size={16} /><span>TripoSR reconstructs a single object locally. Use a sharp image with one unobstructed object; transparent backgrounds also work. Back surfaces are inferred and results vary. Refine adjusts extraction, fragments and smoothing. Retopology and texture baking remain planned. Workspace Demo generates a procedural sculpture.</span></div></>}
    </Dialog>}
    {toast && <div className="toast" role="status"><Info size={15} /><span>{toast}</span><button className="icon-button" aria-label="Dismiss notification" onClick={() => setToast(null)}><X size={13} /></button></div>}
  </div>;
}

function Segments({ options, value, onChange, disabled = false }: { options: string[]; value: string; onChange: (value: string) => void; disabled?: boolean }) { return <div className="segments">{options.map((option) => <button key={option} disabled={disabled} aria-pressed={value === option} className={value === option ? 'selected' : ''} onClick={() => onChange(option)}>{option}</button>)}</div>; }
function ToolButton({ label, icon: Icon, active, onClick }: { label: string; icon: typeof Box; active?: boolean; onClick: () => void }) { return <button className={active ? 'active' : ''} title={label} aria-label={label} onClick={onClick}><Icon size={16} strokeWidth={1.5} /></button>; }
function Toggle({ label, enabled, onToggle, disabled = false }: { label: string; enabled: boolean; onToggle: () => void; disabled?: boolean }) { return <button className={`toggle ${enabled ? 'enabled' : ''}`} role="switch" aria-checked={enabled} aria-label={label} disabled={disabled} onClick={onToggle}><span /></button>; }
function StackRow({ number, icon: Icon, label, subtitle, status, active, enabled, onToggle }: { number: string; icon: typeof Box; label: string; subtitle: string; status: string; active?: boolean; enabled?: boolean; onToggle?: () => void }) { return <div className={`stack-row ${active ? 'active' : ''} ${enabled === false ? 'disabled-stage' : ''}`}><span className={`stack-node ${status === 'complete' ? 'complete' : ''}`}>{status === 'complete' ? <Check size={13} /> : <Icon size={14} />}</span><div className="stack-row-copy"><strong>{label}</strong><span>{subtitle}</span></div>{onToggle ? <Toggle label={`Enable ${label}`} enabled={!!enabled} onToggle={onToggle} /> : status === 'running' ? <LoaderCircle size={12} className="spin" /> : <span className="stack-number">{status === 'complete' ? 'DONE' : number}</span>}</div>; }
function Dialog({ title, children, onClose }: { title: string; children: React.ReactNode; onClose: () => void }) {
  const dialogRef = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const previous = document.activeElement as HTMLElement | null;
    const dialog = dialogRef.current;
    const focusable = () => Array.from(dialog?.querySelectorAll<HTMLElement>('button:not(:disabled), input, select, [tabindex="0"]') || []);
    focusable()[0]?.focus();
    const trap = (event: KeyboardEvent) => { if (event.key !== 'Tab') return; const elements = focusable(); const first = elements[0], last = elements[elements.length - 1]; if (event.shiftKey && document.activeElement === first) { event.preventDefault(); last?.focus(); } else if (!event.shiftKey && document.activeElement === last) { event.preventDefault(); first?.focus(); } };
    dialog?.addEventListener('keydown', trap);
    return () => { dialog?.removeEventListener('keydown', trap); previous?.focus(); };
  }, []);
  return <div className="dialog-backdrop" onMouseDown={(event) => { if (event.target === event.currentTarget) onClose(); }}><div ref={dialogRef} className="dialog" role="dialog" aria-modal="true" aria-labelledby="dialog-title"><button className="dialog-close icon-button" aria-label="Close dialog" onClick={onClose}><X size={18} /></button><h2 id="dialog-title">{title}</h2>{children}</div></div>;
}
