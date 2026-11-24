# Sculpt Mac preview

Sculpt is a free, MIT-licensed desktop application for local single-object
image reconstruction, foreground correction, cached refinement, texture baking,
and GLB/STL export. This is a usable **Beta** preview for **Apple Silicon
(`aarch64-apple-darwin`, `.app` / `.dmg` only)**, not a claim of
production-quality geometry for every image or parity with commercial
generators. Intel Mac (`x86_64`), Universal, `.pkg`, and `.zip` packages are
deferred/unsupported.

> **Current scope (App Shell vs Local AI Inference):**
> - **Desktop App Shell (`Beta`):** `.app` and `.dmg` bundles (`src-tauri/tauri.macos.conf.json`) use ad-hoc signing (`signingIdentity: "-"`). Clean-machine `.dmg` install/launch validation and Developer ID notarization remain pending.
> - **Local AI Inference (`Beta`):** Retains the Apple Silicon 16 GB+ Metal/MPS TripoSR prototype baseline measured on an M4 MacBook Air (16 GB). Retained in **Beta** until clean-machine retesting on packaged builds.

## Install and create an asset

1. Use an Apple Silicon Mac with at least 16 GB RAM and 5 GB free for engine
   setup. The machine with real reconstruction measurements is an M4 MacBook
   Air with 16 GB; other hardware has not been validated.
2. Open the built `.dmg` (`Sculpt_<version>_aarch64.dmg`, from local build or
   the `Sculpt-macOS-arm64-preview` CI artifact) and copy `Sculpt.app` to
   Applications. These builds use local ad-hoc signing, not Developer ID
   notarization. If macOS blocks a downloaded build you trust, attempt opening
   it once, then use System Settings → Privacy & Security → Open Anyway.
   See [Apple's instructions](https://support.apple.com/en-us/102445).
3. Open Sculpt and select TripoSR. Choose **Install local engine**. First setup
   downloads a pinned Python runtime, packages and model files, verifies their
   hashes, and keeps everything in Sculpt's application-support directory.
   This is several gigabytes and needs internet; allow the installer to finish.
4. Choose **Open Sculpt**. Drop a PNG/JPG/WEBP containing a single clear object.
   Review its foreground mask and paint corrections before generating.
5. Generate and inspect all sides. Missing backs, thin parts and noisy surfaces
   are limitations of single-image reconstruction. Use Refine for extraction,
   fragment removal, smoothing and optional 1K/2K color textures.
6. Export GLB for geometry with material/texture, or STL with an explicit height
   in millimetres. STL has geometry only. Check the result in your target tool;
   STL conversion does not repair the reconstructed surface.

Generation runs offline after setup. Closing and reopening the app preserves
the local library. After entering the workspace once, a ready native runtime
skips setup on the next launch. Use the hardware settings to revisit setup.

## Recovery

- Interrupted install: model downloads automatically retry transient Hub errors
  up to twice using the same pinned request and partial file. If setup still
  fails, retry manually; cached downloads are reused.
- Runtime needs repair: use **Verify / repair**. Your saved library is separate
  from model files. You can open the workspace to inspect/export existing assets.
- Insufficient working memory: close memory-heavy applications and try Draft.
  More extraction detail is not a stronger reconstruction model.
- Failed/cancelled generation: the source remains available. Completed original
  assets and refinements remain separate versions in the local library.

Do not delete the `library` directory to repair an installation: it contains
your sources, masks and saved assets. Packaged builds store data under
`~/Library/Application Support/com.sculpt.desktop/`; developer builds use a
separate `library-development` directory.

## What this preview does not finish

The next substantial quality work is a stronger engine with measured results,
followed by validated CUDA/platform support. OBJ/PLY/3MF, retopology, library
thumbnails/rename/delete, damaged-record recovery, notarization, automatic
updates and wider hardware testing remain unfinished. Blender/slicer import
checks remain unverified. A marketing/download website can document the app;
the browser UI is currently a procedural demo, not web AI reconstruction.

## Developer build

```sh
npm ci
npm test
npm run test:ui
cargo test --manifest-path src-tauri/Cargo.toml --target aarch64-apple-darwin --locked
npm run desktop:build -- --target aarch64-apple-darwin --bundles app,dmg
npm run desktop:verify:macos-arm64
```

The bundle verifier checks the `.app` and `.dmg` payload, `arm64`-only architecture,
local signature and exact packaged resource bytes. It does not assert notarization
or run the neural model. The `macos-arm64` Beta lane on `macos-15` builds and
uploads `Sculpt-macOS-arm64-preview` (Apple Silicon `Sculpt_<version>_aarch64.dmg`
plus `SHA256SUMS.txt`, retained for 14 days for human review only with no automatic
release) after static/privacy hygiene, unit, Rust, Python, and UI suites pass;
it does not download model weights or certify GPU hardware.
