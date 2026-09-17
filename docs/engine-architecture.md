# Engine planning and adapter boundaries

Sculpt currently ships one neural reconstruction adapter: TripoSR through an
isolated Python worker. Its validated desktop target is Apple Silicon, Metal,
and at least 16 GB of physical memory. This is an admission floor, not a promise
that every scene fits in memory; the worker checks available memory separately.

`backend/engines/triposr.json` is a checked-in descriptor. It identifies the
engine, pinned source revision, trusted runtime specification, compiled adapter,
and hardware targets. `src-tauri/src/harness/engines.rs` validates that descriptor
and provides two boundaries:

- `plan(hardware, manifests)` is a pure compatibility function. It never installs
  anything, launches a process, or silently substitutes CPU execution.
- `resolve(engine_id, hardware)` returns a compiled `RuntimeAdapter` and an
  explicit compute device. Native generation should use fresh hardware detection
  here, independent of the profile the interface used to display recommendations.

The registered CUDA and CPU targets are **unvalidated** and remain unavailable.
Their presence documents evaluation candidates; it does not claim NVIDIA,
Windows, Linux, or Intel Mac support. Runtime installation and health checks
remain separate from hardware compatibility. A compatible machine may still
need to install or repair its runtime.

Descriptors cannot contain shell commands or arbitrary worker entry points.
Adapters are a Rust enum reviewed and compiled with the application, and runtime
specification paths are allowlisted. No remotely downloaded descriptor can add
executable behavior. A future update mechanism would need a separate trust and
signing design.

The `licenseIdentifier` field records component metadata. TripoSR's local source
includes an MIT license. This field is not a review of every weight, dependency,
or prospective replacement engine. Additions require their own provenance and
license review; the registry does not declare any other model suitable for
commercial distribution.

## Adding an engine

1. Add a compiled adapter with explicit worker protocol, cancellation, output
   validation, and tests. Engine-specific imports and extraction belong inside
   that adapter rather than in the UI.
2. Add a descriptor and pinned runtime specification. Keep target validation
   false until hardware execution and installation have been exercised.
3. Add compatibility fixtures, installation/repair checks, and reconstruction
   benchmarks on the supported hardware. Only then enable the target.
4. Connect the adapter in the native runtime factory. Keep source identity,
   durable jobs, licensing, asset validation, and progress shared in the harness.

This registry is an incremental boundary, not a completed multi-engine runtime
system. The current Python installer and health checker still implement the
TripoSR layout and dependency lock. Per-engine environment directories, separate
install manifests, device-specific PyTorch packages, and cross-platform process
supervision are future work. The procedural demo remains registered solely for
the existing workspace preview and is explicitly not reconstruction.
