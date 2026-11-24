// Verify the built Windows desktop executable, NSIS per-user installer, binary architectures,
// and packaged resources. Clearly separates native ARM64 (0xaa64) from x64 (0x8664)
// emulation and x86 (0x14c) installer wrapper stubs, and rejects deferred MSI/ZIP bundles.
// No model downloads or inference claims.
import assert from 'node:assert/strict'
import { createHash } from 'node:crypto'
import { existsSync, readFileSync, readdirSync, realpathSync, statSync } from 'node:fs'
import { extname, join, relative, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { verifyArtifactDirectory } from './verify-ci-hygiene.mjs'

const root = resolve(import.meta.dirname, '..')

const PE_EXTENSIONS = new Set(['.exe', '.dll', '.pyd', '.node', '.sys'])
const CARGO_INTERNAL_DIRS = new Set([
  'deps',
  'build',
  '.fingerprint',
  'incremental',
  'examples',
  'bundle',
  'nsis',
])

export function parseArgs(argv) {
  let target = null
  let expectedArch = null
  let positionalPath = null
  for (let i = 0; i < argv.length; i += 1) {
    const token = argv[i]
    if (token === '--target') {
      target = argv[++i] ?? null
    } else if (token.startsWith('--target=')) {
      target = token.slice('--target='.length)
    } else if (token === '--expected-arch') {
      expectedArch = argv[++i] ?? null
    } else if (token.startsWith('--expected-arch=')) {
      expectedArch = token.slice('--expected-arch='.length)
    } else if (!token.startsWith('-')) {
      positionalPath = token
    } else {
      throw new Error(`Unknown option: ${token}`)
    }
  }
  return { target, expectedArch, positionalPath }
}

export function expectedArchFromTarget(target) {
  if (!target) return null
  if (target.startsWith('aarch64-')) return 'arm64'
  if (target.startsWith('x86_64-')) return 'x64'
  return null
}

export function normalizeExpectedArch(arch) {
  if (!arch) return null
  const lower = arch.toLowerCase()
  if (lower === 'amd64' || lower === 'x86_64' || lower === 'x64') return 'x64'
  if (lower === 'aarch64' || lower === 'arm64') return 'arm64'
  return lower
}

export function resolveReleaseDir({ target, positionalPath }, rootDir = root) {
  if (positionalPath) {
    return resolve(positionalPath)
  }
  if (target) {
    return join(rootDir, 'src-tauri', 'target', target, 'release')
  }
  const hostTriple = process.arch === 'arm64' ? 'aarch64-pc-windows-msvc' : 'x86_64-pc-windows-msvc'
  const otherTriple = process.arch === 'arm64' ? 'x86_64-pc-windows-msvc' : 'aarch64-pc-windows-msvc'
  const candidates = [
    join(rootDir, 'src-tauri', 'target', hostTriple, 'release'),
    join(rootDir, 'src-tauri', 'target', 'release'),
    join(rootDir, 'src-tauri', 'target', otherTriple, 'release'),
  ]
  return candidates.find(dir => existsSync(join(dir, 'sculpt.exe'))) ?? candidates[0]
}

export function inspectPeBuffer(bytes, filePath = '<buffer>') {
  assert.ok(bytes.length >= 0x40, `PE file is truncated: ${filePath}`)
  assert.equal(bytes.subarray(0, 2).toString('ascii'), 'MZ', `${filePath} is missing DOS MZ header`)
  const peOffset = bytes.readUInt32LE(0x3c)
  assert.ok(peOffset + 6 <= bytes.length, `Invalid PE header offset in ${filePath}`)
  assert.equal(
    bytes.subarray(peOffset, peOffset + 4).toString('ascii'),
    'PE\0\0',
    `${filePath} is missing PE signature`,
  )
  const machine = bytes.readUInt16LE(peOffset + 4)
  const machineHex = `0x${machine.toString(16)}`
  const architecture =
    machine === 0xaa64
      ? 'arm64'
      : machine === 0x8664
        ? 'x64'
        : machine === 0x14c
          ? 'x86'
          : `unknown(${machineHex})`
  const sha256 = createHash('sha256').update(bytes).digest('hex')
  return {
    path: filePath,
    bytes: bytes.length,
    machine,
    machineHex,
    architecture,
    sha256,
  }
}

export function inspectPeBinary(filePath) {
  return inspectPeBuffer(readFileSync(filePath), filePath)
}

export function collectPackagedBinaries(baseDir) {
  const found = []
  function walk(dir, isRoot) {
    for (const entry of readdirSync(dir, { withFileTypes: true })) {
      if (isRoot && entry.isDirectory() && CARGO_INTERNAL_DIRS.has(entry.name)) {
        continue
      }
      const fullPath = join(dir, entry.name)
      if (entry.isDirectory()) {
        walk(fullPath, false)
      } else if (entry.isFile() && PE_EXTENSIONS.has(extname(entry.name).toLowerCase())) {
        found.push(fullPath)
      }
    }
  }
  walk(baseDir, true)
  return found.sort()
}

export function expectedResourceFiles(rootDir = root) {
  return [
    'LICENSE',
    'backend/TEXTURE-NOTICES.txt',
    'backend/worker.py',
    'backend/setup_runtime.py',
    'backend/download_models.py',
    'backend/requirements.lock',
    'backend/runtime-spec.json',
    ...readdirSync(join(rootDir, 'backend/sculpt_backend'))
      .filter(name => name.endsWith('.py'))
      .map(name => `backend/sculpt_backend/${name}`),
  ]
}

export function verifyWindowsBundleConfig(windowsConfig) {
  assert.deepEqual(
    windowsConfig?.bundle?.targets,
    ['nsis'],
    'tauri.windows.conf.json must target only ["nsis"] (MSI/Store/portable ZIP are deferred)',
  )
  assert.equal(
    windowsConfig?.bundle?.windows?.nsis?.installMode,
    'currentUser',
    'tauri.windows.conf.json must configure NSIS installMode: "currentUser"',
  )
  assert.equal(
    windowsConfig?.bundle?.windows?.nsis?.compression,
    'lzma',
    'tauri.windows.conf.json must configure NSIS compression: "lzma"',
  )
  return {
    targets: windowsConfig.bundle.targets,
    installMode: windowsConfig.bundle.windows.nsis.installMode,
    compression: windowsConfig.bundle.windows.nsis.compression,
  }
}

export function verifyWindowsBundle(argv = process.argv.slice(2), rootDir = root) {
  const config = JSON.parse(readFileSync(join(rootDir, 'src-tauri/tauri.conf.json'), 'utf8'))
  const windowsConfig = JSON.parse(readFileSync(join(rootDir, 'src-tauri/tauri.windows.conf.json'), 'utf8'))
  verifyWindowsBundleConfig(windowsConfig)

  const parsed = parseArgs(argv)
  const expectedArch = normalizeExpectedArch(parsed.expectedArch ?? expectedArchFromTarget(parsed.target))
  const releaseDir = resolveReleaseDir(parsed, rootDir)
  const exePath = releaseDir.endsWith('.exe') ? releaseDir : join(releaseDir, 'sculpt.exe')
  const baseDir = releaseDir.endsWith('.exe') ? resolve(exePath, '..') : releaseDir

  assert.ok(existsSync(exePath), `Built Windows executable not found: ${exePath}`)
  const exeInfo = inspectPeBinary(exePath)
  assert.ok(
    exeInfo.machine === 0x8664 || exeInfo.machine === 0xaa64,
    `Unexpected PE machine architecture for sculpt.exe: ${exeInfo.architecture} (${exeInfo.machineHex})`,
  )
  if (expectedArch) {
    assert.equal(
      exeInfo.architecture,
      expectedArch,
      `Expected ${expectedArch} sculpt.exe, found ${exeInfo.architecture} (${exeInfo.machineHex}) at ${exePath}`,
    )
  }

  const packagedBinaries = collectPackagedBinaries(baseDir).map(binPath => {
    const info = inspectPeBinary(binPath)
    const rel = relative(baseDir, binPath).replaceAll('\\', '/')
    return {
      relativePath: rel,
      architecture: info.architecture,
      machineHex: info.machineHex,
      bytes: info.bytes,
      executionModel:
        info.architecture === 'arm64'
          ? 'native-arm64'
          : info.architecture === 'x64'
            ? 'x64-emulation-on-arm64-or-native-x64'
            : 'x86-wow64',
      sha256: info.sha256,
    }
  })

  const foreignArchHelpers = packagedBinaries.filter(bin => bin.architecture !== exeInfo.architecture)
  assert.equal(
    foreignArchHelpers.length,
    0,
    `Packaged payload contains foreign-architecture binaries (${foreignArchHelpers.map(b => `${b.relativePath}:${b.architecture}`).join(', ')})`,
  )

  const hasExternalWebView2Loader = packagedBinaries.some(
    bin => bin.relativePath.toLowerCase() === 'webview2loader.dll',
  )

  const bundleDir = join(baseDir, 'bundle')
  assert.ok(!existsSync(join(bundleDir, 'msi')), 'Unexpected MSI bundle directory; only per-user NSIS EXE is supported')
  if (existsSync(bundleDir)) {
    const unexpectedBundles = readdirSync(bundleDir).filter(name => name !== 'nsis')
    assert.equal(
      unexpectedBundles.length,
      0,
      `Unexpected Windows bundle directory/file(s): ${unexpectedBundles.join(', ')}; only nsis is supported`,
    )
  }

  const nsisDir = join(bundleDir, 'nsis')
  const expectedInstaller = join(
    nsisDir,
    `${config.productName}_${config.version}_${exeInfo.architecture}-setup.exe`,
  )
  assert.ok(existsSync(expectedInstaller), `NSIS installer not found: ${expectedInstaller}`)
  const installerStat = statSync(expectedInstaller)
  assert.ok(installerStat.size > 0, `NSIS installer is empty: ${expectedInstaller}`)
  const installerPe = inspectPeBinary(expectedInstaller)

  const files = expectedResourceFiles(rootDir)
  for (const name of files) {
    const packaged = join(baseDir, name)
    assert.ok(existsSync(packaged), `Packaged resource is missing next to executable: ${packaged}`)
    assert.ok(
      readFileSync(packaged).equals(readFileSync(join(rootDir, name))),
      `Packaged resource is stale: ${name}`,
    )
  }

  verifyArtifactDirectory(baseDir)

  const summary = {
    executable: exeInfo.path,
    version: config.version,
    architecture: exeInfo.architecture,
    machineHex: exeInfo.machineHex,
    nativeArm64Host: exeInfo.architecture === 'arm64',
    executionModel: exeInfo.architecture === 'arm64' ? 'native-arm64' : 'x64',
    executableSha256: exeInfo.sha256,
    installer: expectedInstaller,
    installerMode: windowsConfig.bundle.windows.nsis.installMode,
    installerBytes: installerStat.size,
    installerWrapperArchitecture: installerPe.architecture,
    installerWrapperMachineHex: installerPe.machineHex,
    installerWrapperNote:
      installerPe.architecture === 'x86'
        ? `NSIS uses an x86 (${installerPe.machineHex}) setup stub wrapper that extracts the ${exeInfo.architecture} (${exeInfo.machineHex}) application payload.`
        : undefined,
    installerSha256: installerPe.sha256,
    webview2Loader: hasExternalWebView2Loader ? 'bundled-dll' : 'statically-linked',
    packagedBinaries,
    foreignArchHelpers,
    verifiedResources: files.length,
    signing: 'Unsigned local/candidate build',
  }

  console.log(JSON.stringify(summary, null, 2))
  return summary
}

function isMainModule() {
  if (!process.argv[1]) return false
  try {
    return realpathSync(resolve(process.argv[1])) === realpathSync(fileURLToPath(import.meta.url))
  } catch {
    return resolve(process.argv[1]) === resolve(fileURLToPath(import.meta.url))
  }
}

if (isMainModule()) {
  verifyWindowsBundle()
}
