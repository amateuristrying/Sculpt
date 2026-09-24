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
- Foreground preview and add/erase brush correction before reconstruction, with immutable source-bound masks and restoration from the library. Mask preparation does not consume a trial generation. See [mask workflow and current evaluation limits](docs/foreground-masks.md).
- Memory estimates and headroom checks before loading the model, plus automatic background removal or explicit background preservation.
- Orbit, pan, zoom, camera reset, grid, lighting, material color, and material/wireframe/points/technical views.
- Draft, balanced, and high geometry detail, an asset processing stack, and live geometry metadata.
- Real cached-scene refinement: extraction resolution 96–256, surface density, small-fragment removal, and Taubin smoothing. Each refinement saves a separate version and reuses the original image inference.
- Persistent local library with source/asset integrity checks, job history, interrupted-job recovery, and reopening after relaunch.
- Offline signed-license verification and one-successful-generation release trial. Refinement and existing-asset export remain available after the trial; development builds are unlimited. Checkout is not implemented.
- Binary glTF (`.glb`) export through the native macOS save dialog. Reconstructed assets contain geometry, normals, linear vertex colors, and an explicit nonmetallic material. Viewport grids, lights, and technical overlays are excluded. Generated UVs and texture maps are not implemented.

## Local runtime setup

Open Sculpt and choose **Install local engine** on the hardware setup screen. The installer prepares its own Python 3.11 environment, the pinned TripoSR engine, PyTorch dependencies, and the reconstruction/background-removal models. A bundled app does not require a separately installed Python, Git, or uv. Allow 5 GB free space and an internet connection for initial setup. The native installer currently targets Apple Silicon Macs with at least 16 GB memory.

Progress represents installation stages, not byte-accurate download percentages. Cancel stops the installer and its child processes. Retry reuses cached downloads; **Verify / repair** checks model hashes and repairs damaged downloads. **Clear downloads** removes disposable package caches while preserving the installed runtime and offline model/config files.

Runtime storage:

- Development (`npm run desktop`): `.sculpt-runtime/` inside the checkout.
- Packaged app: `~/Library/Application Support/com.sculpt.desktop/runtime/`.
- `SCULPT_RUNTIME_DIR` can explicitly override the runtime location for development/testing.
- Sources and jobs live in `~/Library/Application Support/com.sculpt.desktop/library-development/` for development or `library/` for packaged builds. Each contains a versioned `library.json`, sources, masks, and job folders. Development and release trial records are separate.
- Older outputs created before the persistent library remain on disk but are not automatically indexed. Assets without a scene cache need a new generation before Refine is available.

The developer CLI remains available when Python 3 and uv are already installed:

```sh
npm run backend:setup
```

Generation runs offline after setup. Both Rust and Python validate the runtime manifest, pinned revisions, dependency lockfile identity, and recorded file sizes/modification times before accepting a job. Full model checksums run at install/repair. The desktop Metal adapter reports an error if Metal is unavailable; it does not silently switch to CPU or the procedural demo. Browser/Vite previews expose only the explicit Workspace Demo.

The 16 GB profile recommends Balanced geometry. High uses a denser extraction grid and is best on 24 GB or larger Macs; it does not use a more capable AI model. Working-memory estimates are conservative guidance rather than measured total GPU memory. Jobs also check remaining memory headroom before loading weights.

After generation, **Refine geometry** reuses the saved scene to change extraction and cleanup settings. It loads only the surface decoder and keeps each version separately. [Refinement behavior and measured batch optimization](docs/refinement.md) document the controls, limits, and reproducible M4 results. Generation presets remain 96/128/192; Refine can reach 256.

## Verification and benchmarks

See [benchmark procedure and findings](backend/benchmarks/README.md). Unit suites cover the frontend, Python worker, Rust harness, and license issuer. Playwright uses an explicit IPC test double and a self-contained GLB fixture for trial/refinement/reopening/export; an optional test also exercises real generated benchmark meshes. Native process execution and Metal inference are verified separately with opt-in Rust integration tests.

The [real-photo evaluation](backend/evaluation/README.md) uses 34 independently sourced CC0 photographs, with source URLs, license evidence, and exact hashes. `npm run backend:eval -- compare` produces an offline side-by-side HTML report with four views per mesh, timing/memory measurements, and silhouette IoU against a frozen automatic foreground mask. The camera is estimated once from the baseline; this is a regression diagnostic, not a human-ground-truth or complete 3D quality score. Photos and generated artifacts are kept out of Git.

CI uses one cached macOS runner, without weights or GPU inference. Public repositories run on push/PR. Private repositories require manually starting the workflow to avoid automatic macOS minute consumption; that manual run still uses the repository's Actions allowance.

```sh
npm run backend:test
npm run test:ui                       # Workspace/lifecycle tests; requires Google Chrome
SCULPT_BENCHMARK_OUTPUT=backend/outputs/YOUR_RUN npm run test:ui
node --test scripts/issue-license.test.mjs
cargo fmt --manifest-path src-tauri/Cargo.toml --check
SCULPT_TEST_IMAGE=/absolute/path/to/object.jpg cargo test --manifest-path src-tauri/Cargo.toml real_image_runs_through_rust_supervisor -- --ignored --nocapture
```

## Deliberate prototype limits

TripoSR is a single-image, single-object reconstruction model. It infers unseen surfaces, so clear images with one isolated object produce the most useful results. It does not understand a full scene, guarantee semantic identity, or produce production-ready topology. The current adapter writes geometry and vertex colors; texture baking, decimation, retopology, UV processing, and OBJ/STL export remain planned stages. The Workspace Demo is still available, but it is explicitly labelled and never presented as an AI result.

TRELLIS.2, SF3D, MLX, CUDA, ONNX, WebGPU, and native alternatives remain replaceable runtime/engine slots; they are not claimed to be available. The persistent library currently fails closed on damaged or unsupported snapshots; general schema migrations and record quarantine remain future work.

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
- `src-tauri/src/harness/engines.rs` validates checked-in engine descriptors and plans compatible adapters/devices; `catalog.rs` presents those recommendations. See [adapter boundaries](docs/engine-architecture.md).
- `src-tauri/src/harness/mod.rs` owns job lifetime, compatibility, events, cancellation, and runtime selection. `library.rs` commits asset history and trial accounting atomically; `licensing.rs` verifies offline entitlements. See [licensing](docs/licensing.md).
- `src-tauri/src/harness/python.rs` supervises one isolated Python worker per job, enforces a timeout, validates protocol messages and GLB output, and records metrics. `runtime.rs` remains the replaceable adapter contract.
- `setup.rs`, `process.rs`, `paths.rs`, and `cache.rs` share native process supervision, packaged resource resolution, installation, and cache ownership. Installation and inference share a single active-operation lock.
- `backend/runtime-spec.json` pins source/model revisions, model checksums, and quality profiles. `backend/requirements.lock` pins dependency versions and package hashes. Runtime processes remain replaceable behind the harness.
- `src-tauri/src/harness/assets.rs` validates image inputs and keeps worker paths out of the UI. `src/geometry/importedAsset.ts` parses and fits returned GLBs without changing their export bytes.
- `src/geometry/sculpture.ts` owns the explicit demo geometry, metadata, and demo GLB export.

Next priorities are completing the mask quality evaluation (including BiRefNet), measured quality presets, additional export formats, texture baking, and library improvements, using the real-photo evaluation to measure output changes. Cached refinement is functional, but 256-resolution extraction remains substantially slower than 128 and smoothing can remove small details. Reconstruction quality still has substantial limits: noisy surfaces, uneven thin structures, and unreliable occluded/cluttered inputs. Competitive quality and cost parity with commercial generators have not been established. Application assets and studio lighting are local; no Sculpt GPU server or per-generation cloud request is used.
