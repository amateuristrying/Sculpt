# Sculpt Inference Harness

The workspace imports this facade instead of a model SDK. Native builds use Tauri
commands and events; Vite previews use an explicitly labelled M4 fixture and an
explicit Workspace Demo. Native detection failures are never replaced with the
preview fixture.

```text
React workspace → Sculpt Harness → selected runtime adapter → AI engine → local GPU
                         │
                         ├── PythonRuntime → TripoSR (PyTorch / Metal)
                         └── MockRuntime → explicit Workspace Demo
```

`src-tauri/src/harness` owns detection, compatibility, job lifetime, cancellation,
and runtime selection. `InferenceRuntime` is the replaceable Rust adapter boundary.
An adapter may supervise a Python/MLX/PyTorch worker process. ONNX, WebGPU, and native
runtimes are separate options; models do not need to share one execution format.

Engine recommendations are based on detected hardware and runtime health. TripoSR is
the first implemented adapter and requires an installed, pinned runtime manifest.
TRELLIS.2 is rejected as unsupported. The Workspace Demo is available only as an
explicit procedural fixture. Unsupported profiles are rejected again in Rust using
actual detected hardware, even if a caller supplies a different profile to the
recommendation query.

## Image input boundary

Native image import checks file type, byte size, dimensions, decode limits, and a
content hash, then returns a local source asset ID. `GenerationRequest` carries that
opaque ID; Rust resolves it for the selected runtime. The UI never supplies a worker
path or model-specific input schema.

## Installation and runtime health

The frontend requests installation through `installRuntime`, listens for
`sculpt://setup-progress`, and cancels through the shared operation registry. Rust
bootstraps a pinned, checksummed uv executable and supervises the isolated installer.
Python installs a hashed dependency lock, pinned source, and verified model files.
Release resources resolve from the app bundle; runtime files live in Application
Support. The developer CLI uses the same installer scripts.

Rust and Python reject stale or modified runtime manifests. Full model hashes run
at install/repair; lightweight file-identity checks run before jobs. Package cache
clearing is forbidden during an active operation and never targets model/config
caches. One process supervisor enforces cancellation and time limits for both setup
and inference.

## Next adapter integration points

- Extend runtime profiles beyond the current validated Apple Silicon adapter.
- Expand the benchmark beyond the current starter objects and synthetic stress
  inputs. Add perceptual quality ratings rather than treating valid GLBs as likeness scores.
- Add foreground preview/correction and durable projects.
- Add cleanup, retopology, UV, and texture adapters as separate stages.
- Add explicit fallback policy in the harness; never silently claim another engine
  generated the requested result.

AI inference is implemented for the TripoSR adapter when the local runtime is set up.
Model installation is available in the app and the developer CLI; accounts and network
inference do not exist. GLB export is real: reconstructed bytes are validated by Rust
and written only to the path chosen in the native save dialog.
