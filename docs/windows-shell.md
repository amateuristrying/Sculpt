# Sculpt Windows Desktop Shell (x64 & ARM64)

Sculpt provides native Windows desktop application shells for both Windows x64 (`x86_64-pc-windows-msvc`) and Windows ARM64 (`aarch64-pc-windows-msvc`), built with Tauri 2, WebView2, Rust, React, TypeScript, and React Three Fiber.

> **Current scope:** The Windows desktop shells validate native host execution, hardware detection, image import, local library persistence, 3D viewport rendering, Workspace Demo, and GLB/STL export. Local AI reconstruction (TripoSR) is **not claimed as supported** on Windows yet (`Experimental / unverified` on Windows x64; `Unsupported / unverified` on Windows ARM64 with no native-inference claim).

## Prerequisites

- Windows 10 or Windows 11 (x64 or ARM64) with Microsoft Edge WebView2 Runtime
- Node.js 22+
- Stable Rust toolchain with `rustfmt` and the target triple(s):
  - Windows x64: `x86_64-pc-windows-msvc`
  - Windows ARM64: `aarch64-pc-windows-msvc`
- Visual Studio 2022 Build Tools (Desktop development with C++, including ARM64 C++ build tools when targeting `aarch64-pc-windows-msvc`, and Windows SDK)

## Build, test, and package

### Windows ARM64 (`aarch64-pc-windows-msvc`)

```powershell
npm ci
npm test
npm run build
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo test --manifest-path src-tauri/Cargo.toml --target aarch64-pc-windows-msvc --locked
npm run desktop:build -- --target aarch64-pc-windows-msvc --bundles nsis
npm run desktop:verify:windows-arm64
```

Produces the per-user NSIS installer at:

```text
src-tauri/target/aarch64-pc-windows-msvc/release/bundle/nsis/Sculpt_<version>_arm64-setup.exe
```

### Windows x64 (`x86_64-pc-windows-msvc`)

```powershell
npm ci
npm test
npm run build
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo test --manifest-path src-tauri/Cargo.toml --target x86_64-pc-windows-msvc --locked
npm run test:ui
npm run desktop:build -- --target x86_64-pc-windows-msvc --bundles nsis
npm run desktop:verify:windows-x64
```

Produces the per-user NSIS installer at:

```text
src-tauri/target/x86_64-pc-windows-msvc/release/bundle/nsis/Sculpt_<version>_x64-setup.exe
```

## Binary architecture audit and emulation separation

`npm run desktop:verify` (via `scripts/verify-windows-bundle.mjs`) inspects PE/COFF machine headers across the built payload:

- **Host executable (`sculpt.exe`):**
  - ARM64 build: PE machine `0xaa64` (`arm64`, `native-arm64`). Verified on physical Snapdragon Windows 11 ARM64 hardware via Win32 `IsWow64Process2` (`ProcessMachine=0x0`, `NativeMachine=0xaa64`) alongside native ARM64 `msedgewebview2.exe` (`0xaa64`).
  - x64 build: PE machine `0x8664` (`x64`). On Windows ARM64 hardware, an x64 build runs under Windows x64 emulation rather than native ARM64 (`mode=x64-emulation-on-arm64` in `sculpt.log`).
- **Packaged helpers & WebView2:** `WebView2Loader` is statically linked into `sculpt.exe`, and `verify-windows-bundle.mjs` asserts `foreignArchHelpers: []` so no x64 or x86 helper binaries are packaged alongside the ARM64 host. All 21 `LICENSE` and `backend/*.py` / `runtime-spec.json` / `requirements.lock` resources are verified byte-for-byte.
- **NSIS installer wrapper:** NSIS produces an `x86` (`0x14c`) setup/uninstaller stub wrapper (`Sculpt_<version>_<arch>-setup.exe` / `uninstall.exe`) that extracts the native `arm64` (`0xaa64`) or `x64` (`0x8664`) application payload.

## Windows storage and logs

Packaged Windows builds store local data under `%LOCALAPPDATA%\com.sculpt.desktop\`:

- `library\` — versioned local asset library (`library.json`, `.library.lock`, `sources\`, `jobs\`)
- `library-development\` — separate library directory used by debug/dev builds
- `logs\sculpt.log` — bounded privacy-safe desktop shell lifecycle log (rotates to `sculpt.prev.log` at 2 MB; records `process`, `host`, and `mode` such as `native-arm64` vs `x64-emulation-on-arm64`, while excluding user images, masks, prompts, and personal file names)
- `runtime\` — reserved for local inference runtime files (`%LOCALAPPDATA%\com.sculpt.desktop\runtime\` in packaged builds, `.sculpt-runtime\` in debug builds, or `SCULPT_RUNTIME_DIR`)

The NSIS installer uses `currentUser` install mode (installing to `%LOCALAPPDATA%\Sculpt` without requiring administrator elevation) and leaves user library/log data intact on uninstall unless manually removed.
