# Sculpt Windows x64 Desktop Shell

Sculpt provides a native Windows x64 desktop application shell built with Tauri 2, WebView2, Rust, React, TypeScript, and React Three Fiber.

> **Current scope:** The Windows x64 build validates the desktop shell, hardware detection, image import, local library persistence, 3D viewport, Workspace Demo, and GLB/STL export. Local AI reconstruction (TripoSR) on Windows is still **Experimental / unverified** and is not claimed as supported yet.

## Prerequisites

- Windows 10 or Windows 11 (x64) with Microsoft Edge WebView2 Runtime
- Node.js 22+
- Stable Rust toolchain with the `x86_64-pc-windows-msvc` target and `rustfmt`
- Visual Studio 2022 Build Tools (Desktop development with C++ and Windows SDK)

## Build, test, and package

```powershell
npm ci
npm test
npm run build
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo test --manifest-path src-tauri/Cargo.toml --target x86_64-pc-windows-msvc --locked
npm run test:ui
npm run desktop:build -- --target x86_64-pc-windows-msvc --bundles nsis
npm run desktop:verify
```

The NSIS per-user installer is produced at:

```text
src-tauri/target/x86_64-pc-windows-msvc/release/bundle/nsis/Sculpt_<version>_x64-setup.exe
```

`npm run desktop:verify` inspects the built `sculpt.exe` PE/COFF architecture (`x64`), verifies the NSIS installer exists, checks SHA-256 hashes, and verifies that all bundled `LICENSE` and `backend/` files match the repository byte-for-byte.

## Windows storage and logs

Packaged Windows builds store local data under `%LOCALAPPDATA%\com.sculpt.desktop\`:

- `library\` — versioned local asset library (`library.json`, `.library.lock`, `sources\`, `jobs\`)
- `library-development\` — separate library directory used by debug/dev builds
- `logs\sculpt.log` — bounded privacy-safe desktop shell lifecycle log (rotates to `sculpt.prev.log` at 2 MB; excludes user images, masks, prompts, and personal file names)
- `runtime\` — reserved for local inference runtime files (`%LOCALAPPDATA%\com.sculpt.desktop\runtime\` in packaged builds, `.sculpt-runtime\` in debug builds, or `SCULPT_RUNTIME_DIR`)

The NSIS installer uses `currentUser` install mode (installing to `%LOCALAPPDATA%\Sculpt` without requiring administrator elevation) and leaves user library/log data intact on uninstall unless manually removed.
