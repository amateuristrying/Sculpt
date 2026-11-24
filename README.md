# Sculpt

A local-first desktop application for an image-to-3D asset pipeline: **Generate → Refine → Texture → Export**.

**Your hardware. Your files. Your inference.** Tauri 2 and Rust own the desktop application; React, TypeScript, and React Three Fiber provide the workspace. No Electron, cloud inference, accounts, or generation API is used.

## Platform and package matrix

Sculpt explicitly separates **Desktop App Shell** validation (installer, native launch, hardware detection, image import, local library persistence, Workspace Demo, and GLB/STL export) from **Local AI Inference** validation:

| Platform / arch | Package format | Desktop App Shell | Local AI Inference |
|---|---|---|---|
| Windows x64 (`x86_64-pc-windows-msvc`) | NSIS per-user EXE (`*_x64-setup.exe`) | Verified (build/install/launch/uninstall tested under x64 emulation; native x64 CI/hardware run pending) | **Experimental / unverified** (TripoSR CUDA/CPU pending validation) |
| Windows ARM64 (`aarch64-pc-windows-msvc`) | Conditional NSIS per-user EXE (`*_arm64-setup.exe`) — **Beta** | Verified natively on physical Snapdragon Windows 11 ARM64 (`mode=native-arm64`) | **Unsupported / unverified** (no native ARM64 AI inference claim) |
| Ubuntu 22.04 LTS x64 (`x86_64-unknown-linux-gnu`) | `.deb` only (`*_amd64.deb`) — **Beta** | **Pending** clean Ubuntu 22.04 runner/VM `.deb` build, install, and launch | **Experimental / unverified** on CUDA; **Unsupported** on CPU-only |
| macOS Apple Silicon (`aarch64-apple-darwin`) | `.app` / `.dmg` (`*_aarch64.dmg`) — **Beta** | **Beta** (ad-hoc signed; clean Mac `.dmg` retest and notarization pending) | **Beta** (Apple Silicon 16 GB+ Metal/MPS TripoSR prototype baseline) |

Deferred / unsupported packages: Windows MSI, system-wide installer, Store/MSIX, and portable ZIP; Linux AppImage, RPM, Snap, Flatpak, Arch, and non-Ubuntu distributions; macOS Intel (`x86_64`), Universal, `.pkg`, and `.zip`. WSL2 Ubuntu is a developer environment only with no standalone desktop package.

## Run on Windows (desktop shell, x64 & ARM64)

Install Node.js 22+, a current stable Rust toolchain (`x86_64-pc-windows-msvc` and/or `aarch64-pc-windows-msvc`), and Visual Studio 2022 Build Tools (Desktop development with C++). From the project directory:

```powershell
npm install
npm test
cargo test --manifest-path src-tauri/Cargo.toml --target aarch64-pc-windows-msvc
npm run desktop:build -- --target aarch64-pc-windows-msvc --bundles nsis
npm run desktop:verify:windows-arm64
```

Replace `aarch64-pc-windows-msvc` / `windows-arm64` with `x86_64-pc-windows-msvc` / `windows-x64` when targeting Windows x64. This produces a per-user NSIS installer (`Sculpt_<version>_arm64-setup.exe` or `Sculpt_<version>_x64-setup.exe`) and stores local library and shell log data under `%LOCALAPPDATA%\com.sculpt.desktop\` (`library\`, `library-development\`, `logs\`). On Windows, local AI reconstruction (TripoSR) remains unvalidated and is not yet claimed as supported; the desktop shells run native host execution, hardware detection, native image import, local library persistence, the Workspace Demo, and GLB/STL export. See [Windows desktop shell notes (x64 & ARM64)](docs/windows-shell.md).

## Run on Ubuntu 22.04 LTS x64 (desktop shell, `.deb`)

Ubuntu 22.04 LTS x64 (`.deb` only) is the sole initial Linux target; AppImage, RPM, Snap, Flatpak, and other Linux distributions are not supported. Install Node.js 22+, a current stable Rust toolchain (`x86_64-unknown-linux-gnu`), and Tauri's WebKitGTK 4.1 system prerequisites (`libwebkit2gtk-4.1-dev`, `build-essential`, `curl`, `wget`, `file`, `libxdo-dev`, `libssl-dev`, `libayatana-appindicator3-dev`, `librsvg2-dev`, `patchelf`). From the project directory on Ubuntu 22.04:

```sh
npm ci
npm test
npm run desktop:build -- --target x86_64-unknown-linux-gnu --bundles deb
npm run desktop:verify:linux-x64
```

This produces `Sculpt_<version>_amd64.deb` under `src-tauri/target/x86_64-unknown-linux-gnu/release/bundle/deb/` and stores local library and shell log data under `~/.local/share/com.sculpt.desktop/` (`library/`, `library-development/`, `logs/`). On Linux, local AI reconstruction (TripoSR) remains unvalidated and is not yet claimed as supported; clean Ubuntu 22.04 VM/CI `.deb` build, install, and launch verification remain pending an Ubuntu 22.04 host/runner. See [Ubuntu 22.04 desktop shell notes](docs/linux-shell.md).

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

Native bundles appear under `src-tauri/target/aarch64-apple-darwin/release/bundle/` (or `src-tauri/target/release/bundle/` when no `--target` is passed). Local builds use ad-hoc signing. Developer ID signing and notarization remain release work. Only Apple Silicon (`aarch64-apple-darwin`) `.app` and `.dmg` bundles are configured (`src-tauri/tauri.macos.conf.json`); Intel Mac, Universal, `.pkg`, and `.zip` packages are deferred. `npm run dev` is available for UI development at `http://127.0.0.1:1420`, using a clearly labelled M4 hardware fixture. It does not detect the browser host's actual hardware.

See [Mac preview installation and first generation](docs/mac-preview.md) for the usable desktop workflow and current release limits. `npm run desktop:verify:macos-arm64` checks the built `.app` and `.dmg` architecture (`arm64` only), local signature, version, and bundled Python/installer/license files. Successful public CI runs also provide a `Sculpt-macOS-arm64-preview` download containing the Apple Silicon `.dmg` (`Sculpt_<version>_aarch64.dmg`) and `SHA256SUMS.txt`; model weights are installed separately on first use.

## What works

- Native hardware inspection and a machine-specific engine planning catalog.
- Native PNG, JPG, and WEBP import into a Rust-owned source registry with size/type/dimension checks and SHA-256 identity.
- Local TripoSR reconstruction on Apple Silicon/Metal through an isolated Python worker, with cancellable progress, GLB validation, vertex colors, and real asset loading in the viewport.
- In-app runtime installation, cancellation, repair, checksum verification, and disposable package-cache clearing.
- Foreground preview and add/erase brush correction before reconstruction, with immutable source-bound masks and restoration from the library. Mask preparation runs independently of reconstruction. See [mask workflow and current evaluation limits](docs/foreground-masks.md).
- Memory estimates and headroom checks before loading the model, plus automatic background removal or explicit background preservation.
- Orbit, pan, zoom, camera reset, grid, lighting, material color, and material/wireframe/points/technical views.
- Draft, balanced, and high geometry detail, an asset processing stack, and live geometry metadata.
- Real cached-scene refinement: extraction resolution 96–256, surface density, small-fragment removal, and Taubin smoothing. Each refinement saves a separate version and reuses the original image inference.
- Persistent local library with source/asset integrity checks, job history, interrupted-job recovery, and reopening after relaunch.
- Free local generation, refinement, and export in both development and packaged builds, with no activation or generation quota. Sculpt code is [MIT licensed](LICENSE); model weights and dependencies retain their own licenses.
- Binary glTF (`.glb`) export through the native macOS save dialog. Reconstructed assets contain geometry, normals, linear vertex colors, and an explicit nonmetallic material. Refine can optionally pack UVs with MIT xatlas, reduce faces with MIT fast-simplification, and embed a 1K or 2K color texture in the GLB. Viewport grids, lights, and technical overlays are excluded.
- Native binary STL export for saved desktop assets, with an explicit object height in millimetres. The export applies a uniform scale, converts the mesh to Z-up, centers it on X/Y, and preserves triangle connectivity; it does not repair open or disconnected geometry.

## Local runtime setup

Open Sculpt and choose **Install local engine** on the hardware setup screen. The installer prepares its own Python 3.11 environment, the pinned TripoSR engine, PyTorch dependencies, and the reconstruction/background-removal models. A bundled app does not require a separately installed Python, Git, or uv. Allow 5 GB free space and an internet connection for initial setup. The native installer currently targets Apple Silicon Macs with at least 16 GB memory.

After opening the native workspace once, subsequent launches skip setup when the remembered reconstruction engine is still compatible and the runtime is ready. A missing or damaged runtime returns to setup. The browser preview and procedural demo do not bypass setup. Hardware settings remain available from the workspace; saved assets can still be opened and exported when the model runtime needs repair.

Progress represents installation stages, not byte-accurate download percentages. Cancel stops the installer and its child processes. Hub model downloads retry transient connection failures, timeouts, HTTP 429 and server errors up to twice, retaining partial downloads and pinned revisions. Other errors still stop setup. Manual Retry also reuses cached downloads; **Verify / repair** checks model hashes and repairs damaged downloads. **Clear downloads** removes disposable package caches while preserving the installed runtime and offline model/config files.

Runtime storage:

- Development (`npm run desktop`): `.sculpt-runtime/` inside the checkout.
- Packaged app: `~/Library/Application Support/com.sculpt.desktop/runtime/`.
- `SCULPT_RUNTIME_DIR` can explicitly override the runtime location for development/testing.
- Sources and jobs live in `~/Library/Application Support/com.sculpt.desktop/library-development/` for development or `library/` for packaged builds. Each contains a versioned `library.json`, sources, masks, and job folders. Development and packaged builds use separate libraries. Version 1 libraries migrate automatically to version 2, preserving sources, assets, and refinement history while removing obsolete activation and trial records. Older Sculpt builds cannot read version 2 libraries; back up the library before downgrading.
- Older outputs created before the persistent library remain on disk but are not automatically indexed. Assets without a scene cache need a new generation before Refine is available.

The developer CLI remains available when Python 3 and uv are already installed:

```sh
npm run backend:setup
```

Generation runs offline after setup. Both Rust and Python validate the runtime manifest, pinned revisions, dependency lockfile identity, and recorded file sizes/modification times before accepting a job. Full model checksums run at install/repair. The desktop Metal adapter reports an error if Metal is unavailable; it does not silently switch to CPU or the procedural demo. Browser/Vite previews expose only the explicit Workspace Demo.

The 16 GB profile recommends Balanced geometry. High uses a denser extraction grid and is best on 24 GB or larger Macs; it does not use a more capable AI model. Working-memory estimates are conservative guidance rather than measured total GPU memory. Jobs also check remaining memory headroom before loading weights.

After generation, **Refine geometry** reuses the saved scene to change extraction and cleanup settings. It loads only the surface decoder and keeps each version separately. [Refinement behavior and measured batch optimization](docs/refinement.md) document the controls, limits, and reproducible M4 results. Generation presets remain 96/128/192; Refine can reach 256.

## Verification and benchmarks

See [benchmark procedure and findings](backend/benchmarks/README.md). Unit suites cover the frontend, Python worker, and Rust harness, including migration from exhausted trial libraries. Playwright uses an explicit IPC test double and a self-contained GLB fixture for repeated generation, refinement, reopening, and export; an optional test also exercises real generated benchmark meshes. Native process execution and Metal inference are verified separately with opt-in Rust integration tests.

The [real-photo evaluation](backend/evaluation/README.md) uses 34 independently sourced CC0 photographs, with source URLs, license evidence, and exact hashes. `npm run backend:eval -- compare` produces an offline side-by-side HTML report with four views per mesh, timing/memory measurements, and silhouette IoU against a frozen automatic foreground mask. The camera is estimated once from the baseline; this is a regression diagnostic, not a human-ground-truth or complete 3D quality score. Photos and generated artifacts are kept out of Git.

The [mask evaluation](docs/foreground-masks.md) compares U2Net and an optional, separately downloaded MIT BiRefNet model on all 34 photos. U2Net remains the default: observed median CPU worker times were 1.20 s versus 19.35 s, with mixed selections and no independent accuracy ground truth yet. A real corrected-mask reconstruction isolates the wooden-horse test subject; it does not establish a general reconstruction-quality improvement.

CI runs `.github/workflows/ci.yml` with read-only `contents: read` permissions and offline model-hub environment guards (`HF_HUB_OFFLINE=1`, `TRANSFORMERS_OFFLINE=1`), without model weights, GPU inference, or automatic release/deploy steps. The workflow checks repository privacy/ignore hygiene (`npm run ci:hygiene`) alongside cross-platform jobs for Windows x64 (`windows-latest`), Windows ARM64 Beta (`windows-11-arm`), Ubuntu 22.04 x64 Beta (`ubuntu-22.04`, `.deb` only), and macOS ARM64 Beta (`macos-15`): each platform lane runs frontend tests/type-check build, Rust `fmt`/`clippy`/`check`/`test --locked`, Python syntax/protocol checks (full locked CPU unit tests on macOS), Playwright UI tests (on `windows-x64`, `ubuntu-x64`, and `macos-arm64`), bundle verification, and artifact hygiene checks with `SHA256SUMS.txt`. Candidate artifacts (`Sculpt-Windows-x64-shell`, `Sculpt-Windows-arm64-shell`, `Sculpt-Ubuntu-22.04-x64-shell`, `Sculpt-macOS-arm64-preview`) are retained for 14 days for human review only. Public repositories run on push/PR; private repositories require manually starting the workflow (`workflow_dispatch`).

```sh
npm run ci:hygiene
npm run backend:test
npm run test:ui                       # Workspace/lifecycle tests; requires Google Chrome
SCULPT_BENCHMARK_OUTPUT=backend/outputs/YOUR_RUN npm run test:ui
cargo fmt --manifest-path src-tauri/Cargo.toml --check
SCULPT_TEST_IMAGE=/absolute/path/to/object.jpg cargo test --manifest-path src-tauri/Cargo.toml real_image_runs_through_rust_supervisor -- --ignored --nocapture
```

## Deliberate prototype limits

TripoSR is a single-image, single-object reconstruction model. It infers unseen surfaces, so clear images with one isolated object produce the most useful results. It does not understand a full scene, guarantee semantic identity, or produce production-ready topology. The current adapter writes geometry and vertex colors; optional texture baking and face reduction are available during cached Refine. Native STL export is available for saved assets; OBJ/PLY/3MF, quad retopology, and production-grade repair remain planned stages. The Workspace Demo is still available, but it is explicitly labelled and never presented as an AI result.

TRELLIS.2, SF3D, MLX, CUDA, ONNX, WebGPU, and native alternatives remain replaceable runtime/engine slots; they are not claimed to be available. The persistent library currently fails closed on damaged or unsupported snapshots; migration from the old trial schema is supported, but general schema migrations and record quarantine remain future work.

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
- `src-tauri/src/harness/mod.rs` owns job lifetime, compatibility, events, cancellation, and runtime selection. `library.rs` commits asset history atomically and migrates libraries from the old trial schema. There is no paid entitlement check.
- `src-tauri/src/harness/python.rs` supervises one isolated Python worker per job, enforces a timeout, validates protocol messages and GLB output, and records metrics. `runtime.rs` remains the replaceable adapter contract.
- `setup.rs`, `process.rs`, `paths.rs`, and `cache.rs` share native process supervision, packaged resource resolution, installation, and cache ownership. Installation and inference share a single active-operation lock.
- `backend/runtime-spec.json` pins source/model revisions, model checksums, and quality profiles. `backend/requirements.lock` pins dependency versions and package hashes. Runtime processes remain replaceable behind the harness.
- `src-tauri/src/harness/assets.rs` validates image inputs and keeps worker paths out of the UI. `src/geometry/importedAsset.ts` parses and fits returned GLBs without changing their export bytes.
- `src/geometry/sculpture.ts` owns the explicit demo geometry, metadata, and demo GLB export.

The cached-scene texture baker is implemented; see [measured results and limits](docs/texture-baking.md). Native STL export is covered by [its format and verification notes](docs/stl-export.md). Next priorities are CUDA support and evaluation of a stronger second engine. OBJ/PLY/3MF, library thumbnails and quarantine of damaged records remain planned. Output changes will be compared on the existing real-photo evaluation set; further TripoSR preset tuning is not the focus. Cached refinement is functional, but 256-resolution extraction remains substantially slower than 128 and smoothing can remove small details. Reconstruction quality still has substantial limits: noisy surfaces, uneven thin structures, and unreliable occluded/cluttered inputs. Competitive quality and cost parity with commercial generators have not been established. Application assets and studio lighting are local; no Sculpt GPU server or per-generation cloud request is used.
