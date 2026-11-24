# Sculpt Ubuntu 22.04 LTS x64 Desktop Shell (`.deb`)

**Ubuntu 22.04 LTS x64 (`x86_64-unknown-linux-gnu`, `.deb` package)** is the sole initial Linux target for Sculpt, built with Tauri 2, WebKitGTK 4.1, Rust, React, TypeScript, and React Three Fiber.

> **Current scope and status:**
> - **Packaging target:** `.deb` only (`src-tauri/tauri.linux.conf.json`). AppImage, RPM, Snap, Flatpak, Arch, and other Linux distributions are **not supported or packaged**.
> - **AI inference status:** Local AI reconstruction (TripoSR) is **not claimed as supported** on Linux yet (`Experimental / unverified` when NVIDIA CUDA is detected; `Unsupported` on CPU-only machines; Workspace Demo is recommended).
> - **Verification status:** Tauri `.deb` packaging configuration, Linux hardware discovery/engine catalog unit tests, `.deb` bundle verifier unit tests, Playwright UI smoke tests, CI/artifact hygiene checks (`npm run ci:hygiene`), and the `ubuntu-22.04` GitHub Actions job (`Sculpt-Ubuntu-22.04-x64-shell` with `SHA256SUMS.txt`, retained 14 days for human review with no auto-release) are implemented. Clean Ubuntu 22.04 VM / CI `.deb` build, install, and desktop launch verification remain **pending** execution on an Ubuntu 22.04 host/runner.

## System prerequisites (Ubuntu 22.04 LTS x64)

### Build prerequisites

Building the Tauri 2 desktop shell from source on Ubuntu 22.04 LTS requires WebKitGTK 4.1 development headers and native build tools:

```sh
sudo apt-get update
sudo apt-get install -y \
  libwebkit2gtk-4.1-dev \
  build-essential \
  curl \
  wget \
  file \
  libxdo-dev \
  libssl-dev \
  libayatana-appindicator3-dev \
  librsvg2-dev \
  patchelf
```

Also install:
- Node.js 22+
- Stable Rust toolchain with `rustfmt` and target `x86_64-unknown-linux-gnu`

### Runtime dependencies

The `.deb` package relies on Ubuntu 22.04 standard WebKitGTK 4.1 and GTK 3 libraries (`libwebkit2gtk-4.1-0`, `libgtk-3-0`), which `apt` resolves automatically when installing the `.deb` package.

## Build, verify, and install on Ubuntu 22.04

```sh
npm ci
npm test
npm run build
cargo fmt --manifest-path src-tauri/Cargo.toml --check
cargo test --manifest-path src-tauri/Cargo.toml --target x86_64-unknown-linux-gnu --locked
npm run desktop:build -- --target x86_64-unknown-linux-gnu --bundles deb
npm run desktop:verify:linux-x64
```

This produces the Debian package at:

```text
src-tauri/target/x86_64-unknown-linux-gnu/release/bundle/deb/Sculpt_<version>_amd64.deb
```

Install, launch, and remove cleanly on Ubuntu 22.04:

```sh
sudo apt-get install -y ./src-tauri/target/x86_64-unknown-linux-gnu/release/bundle/deb/Sculpt_<version>_amd64.deb
sculpt
sudo apt-get remove -y sculpt
```

## Bundle verification (`scripts/verify-linux-bundle.mjs`)

`npm run desktop:verify:linux-x64` verifies:

- The compiled `sculpt` binary is an ELF64 `x86_64` (`0x3e`) executable with zero foreign-architecture ELF helper binaries.
- Only `.deb` bundles are produced (rejecting unexpected `appimage/` or `rpm/` bundle directories).
- `dpkg-deb -f` reports matching version, `amd64` architecture, and WebKitGTK 4.1 (`libwebkit2gtk-4.1-0`) + GTK 3 (`libgtk-3-0`) dependencies (rejecting legacy WebKitGTK 4.0).
- `dpkg-deb -c` confirms `/usr/bin/sculpt`, the `.desktop` entry under `/usr/share/applications/`, PNG hicolor icons under `/usr/share/icons/hicolor/`, and all 21 `LICENSE` and `backend/` resources.

## Linux storage and logs

Packaged Linux builds follow XDG user directories (`$XDG_DATA_HOME/com.sculpt.desktop/`, defaulting to `~/.local/share/com.sculpt.desktop/`):

- `library/` — versioned local asset library (`library.json`, `.library.lock`, `sources/`, `jobs/`)
- `library-development/` — separate library directory used by debug/dev builds
- `logs/sculpt.log` — bounded privacy-safe desktop shell lifecycle log (rotates to `sculpt.prev.log` at 2 MB; excludes user images, masks, prompts, and personal file names)
- `runtime/` — reserved for local inference runtime files (`~/.local/share/com.sculpt.desktop/runtime/` in packaged builds, `.sculpt-runtime/` in debug builds, or `SCULPT_RUNTIME_DIR`)
