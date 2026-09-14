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
- Local PNG, JPG, and WEBP selection, a source preview, cancellable generation progress, and a procedural 3D asset.
- Orbit, pan, zoom, camera reset, grid, lighting, material color, and material/wireframe/points/technical views.
- Draft, balanced, and high geometry detail, an asset processing stack, and live geometry metadata.
- Binary glTF (`.glb`) export through the native macOS save dialog. The file contains asset geometry, UVs, and a material; viewport grids, lights, and technical overlays are excluded.

## Deliberate prototype limits

Generation is **simulated**. The image stays local and acts as a source reference; no image reconstruction or AI inference occurs. The result is a procedural study, with a repeatable variation derived from the image filename. No model is downloaded, installed, or loaded.

SF3D/Apple and the lightweight engine are planning profiles, not functioning model integrations. Their recommendations express hardware suitability targets. The SF3D size is an estimate, and its proposed Apple adapter still requires validation. TRELLIS.2 is unavailable. Mesh cleanup, retopology, UV work, and texture-generation controls describe future stages; enabling them does not execute those algorithms. OBJ and STL are listed as future export formats; GLB is the working format. Projects are session-based rather than a durable project-file system.

## Inference boundary

```text
React workspace
    ↓ SculptHarness interface
Tauri commands / SculptInferenceHarness (Rust)
    ↓ InferenceRuntime adapter
MockRuntime now; MLX / CUDA-PyTorch / ONNX / WebGPU / native later
    ↓ engine + user's hardware
```

- `src/harness/types.ts` is the frontend contract: hardware and engine profiles, runtime/model states, generation requests, progress, cancellation, and asset results.
- `src/harness/index.ts` routes native calls through Tauri and supplies the browser-only design preview.
- `src-tauri/src/harness/hardware.rs` reads macOS `system_profiler`, `sysctl`, `sw_vers`, and `diskutil` results. Missing fields remain unknown. Detected Metal describes a hardware API, not an installed ML runtime; Rosetta does not hide Apple Silicon detection.
- `src-tauri/src/harness/catalog.rs` gates engine recommendations against detected capabilities and memory.
- `src-tauri/src/harness/mod.rs` owns job admission, compatibility validation, events, and cancellation. It currently selects `MockRuntime` explicitly.
- `src-tauri/src/harness/runtime.rs` defines the replaceable runtime adapter. A future implementation can supervise a Python worker or native library without exposing a model SDK to the UI or rewriting the ML ecosystem in Rust. ONNX is one option, not a required conversion target.
- `src/geometry/sculpture.ts` owns the demo geometry, metadata, and GLB export; the viewport uses the same underlying geometry for its visualization modes.

Model download, load/unload, memory policy, runtime selection, and fallbacks belong behind the harness. Their contracts are scaffolding in this version, not completed subsystems. Application assets and studio lighting are local; running the prototype requires no Sculpt GPU server or per-generation network request.
