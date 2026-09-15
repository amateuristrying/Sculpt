import { ArrowRight, Check, ChevronRight, Cpu, HardDrive, LoaderCircle, LockKeyhole, Monitor, ShieldCheck, Zap } from 'lucide-react';
import type { BackendStatus, EngineProfile, HardwareProfile } from '../harness/types';
import { Brand, SculptMark } from './Brand';
import RuntimeSetup, { type RuntimeSetupProps } from './RuntimeSetup';

export default function Setup({ hardware, engines, selectedEngine, onSelect, onContinue, error, onRetry, backend, runtimeSetup }: {
  hardware: HardwareProfile | null;
  backend: BackendStatus | null;
  runtimeSetup: RuntimeSetupProps;
  engines: EngineProfile[];
  selectedEngine: string;
  onSelect: (id: string) => void;
  onContinue: () => void;
  error: string | null;
  onRetry: () => void;
}) {
  const metal = hardware?.computeBackends.some((backend) => backend.toLowerCase().includes('metal'));
  return <main className="setup-screen">
    <header className="setup-header"><Brand /><span className="eyebrow">YOUR LOCAL 3D STUDIO</span><span className="version-tag">EARLY ACCESS <span>0.1</span></span></header>
    <div className="setup-content">
      <div className="setup-intro"><span className="section-kicker"><span className="live-dot" /> ALL CREATIVITY. ALL LOCAL.</span><h1>Your next dimension.<br /><span>Powered by your Mac.</span></h1><p>Preparing your local 3D environment.<br />A little setup. A whole new way to create.</p></div>
      <div className="setup-columns">
        <section className="hardware-card">
          <div className="card-heading"><span>01</span> YOUR HARDWARE <span className="tiny-pill">{hardware?.detectionSource === 'preview' ? 'PREVIEW PROFILE' : 'DEVICE CHECK'}</span></div>
          <div className="chip-illustration"><div className="chip-lines" /><div className="chip-face"><SculptMark size={30} /><span>{hardware?.chip.replace('Apple ', '') || '…'}</span><small>LOCAL COMPUTE</small></div><span className="chip-signal"><Check size={10} /></span></div>
          <h2>{hardware?.chip || 'Getting to know your Mac'}</h2>
          <p className="hardware-subtitle">{hardware ? `${hardware.memoryGb || 'Unknown'} GB ${hardware.unifiedMemory ? 'Unified Memory' : 'RAM'} · ${hardware.gpu}` : 'Checking hardware and compute capabilities…'}</p>
          <div className="hardware-checks">
            <Capability label={hardware?.isAppleSilicon ? 'Apple Silicon' : 'Processor detected'} value={hardware?.architecture || 'Checking'} ready={!!hardware} />
            <Capability label={metal ? 'Metal GPU' : 'Compute backend'} value={metal ? 'Supported' : hardware?.computeBackends.join(', ') || 'Checking'} ready={!!metal} />
            <Capability label="Local generation" value={backend?.installed ? 'Model installed' : 'Setup required'} ready={!!backend?.installed} />
            <Capability label="Engine profile" value={engines.length ? 'Matched to your Mac' : 'Checking'} ready={engines.length > 0} />
          </div>
          <div className="device-details"><span><Monitor size={13} /> {hardware ? `${hardware.os} ${hardware.osVersion}` : 'macOS'}</span><span><HardDrive size={13} /> {hardware?.storageGb ? `${Math.round(hardware.storageGb)} GB storage` : 'Local storage'}</span></div>
          {hardware?.detectionSource === 'preview' && <p className="preview-disclosure">Development preview: Apple M4 / 16 GB test profile. Open the desktop app for real hardware detection.</p>}
        </section>
        <section className="engine-setup">
          <div className="card-heading"><span>02</span> YOUR ENGINE</div><h2>The right fit for your hardware.</h2><p className="engine-intro">One thoughtful starting point. Room to grow.</p>
          <div className="engine-options">{engines.map((engine, index) => <button key={engine.id} className={`engine-option ${selectedEngine === engine.id ? 'selected' : ''} ${engine.compatibility === 'unsupported' ? 'unsupported' : ''}`} aria-pressed={selectedEngine === engine.id} disabled={engine.compatibility === 'unsupported'} onClick={() => onSelect(engine.id)} title={engine.reason}>
            <span className="engine-symbol">{index === 0 ? <Zap size={19} /> : index === 1 ? <Cpu size={19} /> : <SculptMark size={19} />}</span><span className="engine-copy"><span className="engine-title">{engine.name}<span className={`engine-tag ${engine.compatibility === 'recommended' ? 'recommended' : ''}`}>{engine.compatibility === 'recommended' ? 'RECOMMENDED' : engine.compatibility === 'unsupported' ? 'UNSUPPORTED' : 'DEMO'}</span></span><span className="engine-subtitle">{engine.subtitle}</span><span className="engine-description">{engine.description}</span><span className="engine-footnote">{engine.modelSizeGb ? `~${engine.modelSizeGb} GB estimated model` : 'No model download'}<span>·</span>{engine.implemented ? engine.id === 'demo' ? 'No AI' : backend?.installed ? 'Installed' : 'Setup required' : 'Planned adapter'}</span></span><span className="radio-check">{selectedEngine === engine.id && <Check size={12} />}</span>
          </button>)}</div>
          {!hardware && !error && <div className="detecting"><LoaderCircle size={18} className="spin" /> Detecting your environment…</div>}
          {error && <div role="alert" className="error-box">{error}<button onClick={onRetry}>Try again <ChevronRight size={12} /></button></div>}
          <RuntimeSetup {...runtimeSetup} />
        </section>
      </div>
      <div className="setup-bottom"><div><ShieldCheck size={19} /><p><strong>Your hardware. Your files. Your inference.</strong><span>No uploads. No cloud credits. Just your creative process.</span></p></div><button className="primary-button setup-continue" onClick={onContinue} disabled={!hardware || !selectedEngine}>Open Sculpt <ArrowRight size={16} /></button></div>
    </div>
    <footer className="setup-footer"><span><LockKeyhole size={12} /> PRIVATE BY DESIGN</span><span>IMAGE <ChevronRight size={11} /> GEOMETRY <ChevronRight size={11} /> REFINE <ChevronRight size={11} /> EXPORT</span><span>MADE FOR WHAT’S NEXT</span></footer>
  </main>;
}

function Capability({ label, value, ready }: { label: string; value: string; ready: boolean }) {
  return <div className="capability"><span className={ready ? 'check-circle' : 'pending-circle'}>{ready ? <Check size={11} /> : <span />}</span><span>{label}</span><span>{value}</span></div>;
}
