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
- In-app runtime installation, cancellation, repair, checksum verification, and disposable package-cache clearing.
- Memory estimates and headroom checks before loading the model, plus automatic background removal or explicit background preservation.
- Orbit, pan, zoom, camera reset, grid, lighting, material color, and material/wireframe/points/technical views.
- Draft, balanced, and high geometry detail, an asset processing stack, and live geometry metadata.
- Binary glTF (`.glb`) export through the native macOS save dialog. Reconstructed assets contain geometry, normals, linear vertex colors, and an explicit nonmetallic material. Viewport grids, lights, and technical overlays are excluded. Generated UVs and texture maps are not implemented.

## Local runtime setup

Open Sculpt and choose **Install local engine** on the hardware setup screen. The installer prepares its own Python 3.11 environment, the pinned TripoSR engine, PyTorch dependencies, and the reconstruction/background-removal models. A bundled app does not require a separately installed Python, Git, or uv. Allow 5 GB free space and an internet connection for initial setup. The native installer currently targets Apple Silicon Macs with at least 16 GB memory.

Progress represents installation stages, not byte-accurate download percentages. Cancel stops the installer and its child processes. Retry reuses cached downloads; **Verify / repair** checks model hashes and repairs damaged downloads. **Clear downloads** removes disposable package caches while preserving the installed runtime and offline model/config files.

Runtime storage:

- Development (`npm run desktop`): `.sculpt-runtime/` inside the checkout.
- Packaged app: `~/Library/Application Support/com.sculpt.desktop/runtime/`.
- `SCULPT_RUNTIME_DIR` can explicitly override the runtime location for development/testing.
- Imported sources and generated jobs live in Sculpt's Application Support directory. They are local files, but the current project registry remains session-based. Export work you want to keep accessible.

The developer CLI remains available when Python 3 and uv are already installed:

```sh
npm run backend:setup
```

Generation runs offline after setup. Both Rust and Python validate the runtime manifest, pinned revisions, dependency lockfile identity, and recorded file sizes/modification times before accepting a job. Full model checksums run at install/repair. The desktop Metal adapter reports an error if Metal is unavailable; it does not silently switch to CPU or the procedural demo. Browser/Vite previews expose only the explicit Workspace Demo.

The 16 GB profile recommends Balanced geometry. High uses a denser extraction grid and is best on 24 GB or larger Macs; it does not use a more capable AI model. Working-memory estimates are conservative guidance rather than measured total GPU memory. Jobs also check remaining memory headroom before loading weights.

## Verification and benchmarks

See [benchmark procedure and findings](backend/benchmarks/README.md). The current suite has 30 frontend unit tests, 27 Python tests, and 12 Rust unit tests, plus opt-in real GPU/installer integration tests and Playwright workspace tests. The UI integration tests use an explicit IPC test double with actual generated GLBs; native process execution is verified separately in Rust.

```sh
npm run backend:test
npm run test:ui                       # Installer UI; requires Google Chrome
SCULPT_TEST_IMAGE=/absolute/path/to/object.jpg cargo test --manifest-path src-tauri/Cargo.toml real_image_runs_through_rust_supervisor -- --ignored --nocapture
```

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
- `setup.rs`, `process.rs`, `paths.rs`, and `cache.rs` share native process supervision, packaged resource resolution, installation, and cache ownership. Installation and inference share a single active-operation lock.
- `backend/runtime-spec.json` pins source/model revisions, model checksums, and quality profiles. `backend/requirements.lock` pins dependency versions and package hashes. Runtime processes remain replaceable behind the harness.
- `src-tauri/src/harness/assets.rs` validates image inputs and keeps worker paths out of the UI. `src/geometry/importedAsset.ts` parses and fits returned GLBs without changing their export bytes.
- `src/geometry/sculpture.ts` owns the explicit demo geometry, metadata, and demo GLB export.

Next priorities are foreground-mask preview and correction, a broader photographic evaluation set, separate mesh cleanup and texture-baking stages, and durable project files. Reconstruction quality still has substantial limits: noisy surfaces, uneven thin structures, and unreliable occluded/cluttered inputs. Competitive quality and cost parity with commercial generators have not been established. Application assets and studio lighting are local; no Sculpt GPU server or per-generation cloud request is used.
