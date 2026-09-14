# Sculpt

A macOS desktop prototype for a local image-to-3D asset pipeline: **Generate → Refine → Texture → Export**.

**Your hardware. Your files. Your inference.** Tauri 2 and Rust own the desktop application; React, TypeScript, and React Three Fiber provide the workspace. No Electron, cloud inference, accounts, or generation API is used.

## Run on macOS

Install Node.js 22+, a current stable Rust toolchain, and Xcode Command Line Tools (`xcode-select --install`). Then, from the project directory:

```sh
npm install
npm run desktop
```

The initial Cargo build downloads and compiles native dependencies. Later launches are faster. `npm run desktop` starts both the local UI development server and the native Tauri application. The target development profile is an Apple M4 MacBook Air with 16 GB unified memory and 256 GB storage.

```sh
npm test                 # Geometry and harness tests
npm run build            # Type-check and build the UI
cargo test --manifest-path src-tauri/Cargo.toml
npm run desktop:build    # Build the macOS .app and .dmg
```

Native bundles appear under `src-tauri/target/release/bundle/`. Local builds use ad-hoc signing. Developer ID signing and notarization remain release work. To build only the application, run `npm run desktop:build -- --bundles app`. `npm run dev` is available for UI development at `http://127.0.0.1:1420`, using a clearly labelled M4 hardware fixture. It does not detect the browser host's actual hardware.

## What works

- Native hardware inspection and a machine-specific engine planning catalog.
- Native PNG, JPG, and WEBP import into a Rust-owned source registry with size/type/dimension checks and SHA-256 identity.
- Local TripoSR reconstruction on Apple Silicon/Metal through an isolated Python worker, with cancellable progress, GLB validation, vertex colors, and real asset loading in the viewport.
- Orbit, pan, zoom, camera reset, grid, lighting, material color, and material/wireframe/points/technical views.
- Draft, balanced, and high geometry detail, an asset processing stack, and live geometry metadata.
- Binary glTF (`.glb`) export through the native macOS save dialog. The file contains asset geometry, UVs, and a material; viewport grids, lights, and technical overlays are excluded.

## Local runtime setup

The first real adapter is TripoSR. Its Python environment, source checkout, model weights, and U2Net background model are intentionally kept outside Git under `.sculpt-runtime/`. Install them once on the development machine:

```sh
npm run backend:setup
```

This downloads roughly 2 GB of model/runtime data and requires at least 4 GB free. Generation itself runs offline after setup. The desktop app checks the runtime manifest before enabling the TripoSR button; it never silently falls back to the procedural demo. Browser/Vite previews cannot launch local inference and expose only the explicit Workspace Demo.

## Deliberate prototype limits

TripoSR is a single-image, single-object reconstruction model. It infers unseen surfaces, so clear images with one isolated object produce the most useful results. It does not understand a full scene, guarantee semantic identity, or produce production-ready topology. The current adapter writes geometry and vertex colors; texture baking, cleanup, retopology, UV processing, and OBJ/STL export remain planned stages. The Workspace Demo is still available, but it is explicitly labelled and never presented as an AI result.

TRELLIS.2, SF3D, MLX, CUDA, ONNX, WebGPU, and native alternatives remain replaceable runtime/engine slots; they are not claimed to be available. Projects are session-based rather than a durable project-file system.

## Inference boundary

```text
React workspace
    ↓ SculptHarness interface
Tauri commands / SculptInferenceHarness (Rust)
    ↓ InferenceRuntime adapter
PythonRuntime → TripoSR (PyTorch / Metal); MockRuntime → explicit demo
    ↓ engine + user's hardware
```

- `src/harness/types.ts` is the frontend contract: hardware and engine profiles, runtime/model states, generation requests, progress, cancellation, and asset results.
- `src/harness/index.ts` routes native calls through Tauri and supplies the browser-only design preview.
- `src-tauri/src/harness/hardware.rs` reads macOS `system_profiler`, `sysctl`, `sw_vers`, and `diskutil` results. Missing fields remain unknown. Detected Metal describes a hardware API, not an installed ML runtime; Rosetta does not hide Apple Silicon detection.
- `src-tauri/src/harness/catalog.rs` gates engine recommendations against detected capabilities and memory.
- `src-tauri/src/harness/mod.rs` owns source/job registries, compatibility validation, events, cancellation, and explicit runtime selection.
- `src-tauri/src/harness/python.rs` supervises one isolated Python worker per job, enforces a timeout, validates protocol messages and GLB output, and records metrics. `runtime.rs` remains the replaceable adapter contract.
- `src-tauri/src/harness/assets.rs` validates image inputs and keeps worker paths out of the UI. `src/geometry/importedAsset.ts` parses and fits returned GLBs without changing their export bytes.
- `src/geometry/sculpture.ts` owns the explicit demo geometry, metadata, and demo GLB export.

Model download, load/unload, memory policy, runtime selection, and fallbacks belong behind the harness. Download is currently a developer CLI step; an in-app installer, resumable downloads, model checksums, cache eviction, and a larger image benchmark are the next backend milestones. Application assets and studio lighting are local; no Sculpt GPU server or per-generation cloud request is used.
