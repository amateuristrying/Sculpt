export function SculptMark({ size = 26 }: { size?: number }) {
  return <svg width={size} height={size} viewBox="0 0 32 36" fill="none" aria-hidden="true">
    <path d="M28 3H12L3 17h15L28 3Z" fill="currentColor" />
    <path d="M4 33h16l9-14H14L4 33Z" fill="currentColor" opacity=".64" />
  </svg>;
}

export function Brand({ compact = false }: { compact?: boolean }) {
  return <div className={`brand ${compact ? 'compact' : ''}`}><SculptMark size={compact ? 23 : 30} /><span>sculpt<span className="brand-period">.</span></span></div>;
}
