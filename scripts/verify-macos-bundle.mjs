// Verify the built macOS Apple Silicon .app and .dmg bundles, architecture (arm64 only),
// local signature, Info.plist metadata, packaged resources, and absence of Intel/PKG/ZIP bundles.
// No model downloads or inference claims.
import assert from 'node:assert/strict'
import { execFileSync } from 'node:child_process'
import { createHash } from 'node:crypto'
import { existsSync, mkdtempSync, readFileSync, readdirSync, realpathSync, rmSync, statSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { verifyArtifactDirectory } from './verify-ci-hygiene.mjs'

const root = resolve(import.meta.dirname, '..')

export function parseArgs(argv) {
  let target = null
  let expectedArch = null
  let positionalPath = null
  let requireDmg = true
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
    } else if (token === '--app-only') {
      requireDmg = false
    } else if (!token.startsWith('-')) {
      positionalPath = token
    } else {
      throw new Error(`Unknown option: ${token}`)
    }
  }
  return { target, expectedArch, positionalPath, requireDmg }
}

export function expectedArchFromTarget(target) {
  if (!target) return null
  if (target === 'aarch64-apple-darwin' || target.startsWith('aarch64-')) return 'arm64'
  if (target === 'x86_64-apple-darwin' || target === 'universal-apple-darwin') {
    throw new Error(`Unsupported macOS target "${target}"; only Apple Silicon (aarch64-apple-darwin) is in the package matrix`)
  }
  return null
}

export function normalizeExpectedArch(arch) {
  if (!arch) return null
  const lower = arch.toLowerCase()
  if (lower === 'aarch64' || lower === 'arm64') return 'arm64'
  throw new Error(`Unsupported macOS architecture "${arch}"; only Apple Silicon (arm64) is in the package matrix`)
}

export function parseLipoArchs(output) {
  const archs = output
    .trim()
    .split(/\s+/)
    .filter(Boolean)
  assert.deepEqual(
    archs,
    ['arm64'],
    `Expected single-slice Apple Silicon binary (arm64 only), found: ${archs.join(', ') || '(none)'}`,
  )
  return archs
}

export function verifyMacosBundleConfig(baseConfig, macosConfig) {
  assert.deepEqual(
    baseConfig?.bundle?.targets,
    ['app', 'dmg'],
    'tauri.conf.json bundle.targets must be ["app", "dmg"]',
  )
  assert.deepEqual(
    macosConfig?.bundle?.targets,
    ['app', 'dmg'],
    'tauri.macos.conf.json bundle.targets must be ["app", "dmg"] (PKG/ZIP/Intel are deferred)',
  )
  assert.equal(
    macosConfig?.bundle?.macOS?.minimumSystemVersion,
    '12.0',
    'tauri.macos.conf.json must declare macOS.minimumSystemVersion: "12.0"',
  )
  return {
    targets: macosConfig.bundle.targets,
    minimumSystemVersion: macosConfig.bundle.macOS.minimumSystemVersion,
  }
}

export function isExpectedAppleSiliconDmgName(fileName, productName, version) {
  const lower = fileName.toLowerCase()
  if (!lower.endsWith('.dmg')) return false
  if (lower.includes('x86_64') || lower.includes('x64') || lower.includes('universal')) {
    return false
  }
  const prefix = `${productName.toLowerCase()}_${version.toLowerCase()}_`
  return lower === `${prefix}aarch64.dmg` || lower === `${prefix}arm64.dmg`
}

export function verifyMacosBundleLayout(bundleDir, productName, version, { requireDmg = true } = {}) {
  assert.ok(!existsSync(join(bundleDir, 'pkg')), 'Unexpected macOS .pkg bundle directory; only .app and .dmg are supported')

  const entries = existsSync(bundleDir) ? readdirSync(bundleDir) : []
  const unexpectedFiles = entries.filter(name => name.endsWith('.zip') || name.endsWith('.pkg'))
  assert.equal(
    unexpectedFiles.length,
    0,
    `Unexpected macOS bundle file(s): ${unexpectedFiles.join(', ')}; only .app and .dmg are supported`,
  )

  const appPath = join(bundleDir, 'macos', `${productName}.app`)
  assert.ok(existsSync(appPath), `macOS app bundle not found: ${appPath}`)

  let dmgPath = null
  const dmgDir = join(bundleDir, 'dmg')
  if (requireDmg) {
    assert.ok(existsSync(dmgDir), `macOS DMG bundle directory not found: ${dmgDir}`)
    const dmgEntries = readdirSync(dmgDir).filter(name => name.toLowerCase().endsWith('.dmg'))
    assert.ok(dmgEntries.length > 0, `Expected Apple Silicon .dmg inside ${dmgDir}`)
    const invalidDmgs = dmgEntries.filter(name => !isExpectedAppleSiliconDmgName(name, productName, version))
    assert.equal(
      invalidDmgs.length,
      0,
      `Unexpected macOS DMG filename(s) (expected ${productName}_${version}_aarch64.dmg): ${invalidDmgs.join(', ')}`,
    )
    dmgPath = join(dmgDir, dmgEntries[0])
    assert.ok(statSync(dmgPath).size > 0, `macOS DMG is empty: ${dmgPath}`)
  }

  return { appPath, dmgPath }
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

function resolveBundleDir({ target, positionalPath }, rootDir = root) {
  if (positionalPath) {
    const resolved = resolve(positionalPath)
    if (resolved.endsWith('.app')) {
      return resolve(resolved, '..', '..')
    }
    if (resolved.endsWith('bundle')) {
      return resolved
    }
    return join(resolved, 'bundle')
  }
  if (target) {
    return join(rootDir, 'src-tauri', 'target', target, 'release', 'bundle')
  }
  const candidates = [
    join(rootDir, 'src-tauri', 'target', 'aarch64-apple-darwin', 'release', 'bundle'),
    join(rootDir, 'src-tauri', 'target', 'release', 'bundle'),
  ]
  return candidates.find(dir => existsSync(join(dir, 'macos', 'Sculpt.app'))) ?? candidates[0]
}

function verifyAppContents(appPath, config, files, rootDir) {
  const contents = join(appPath, 'Contents')
  const resources = join(contents, 'Resources')
  const plist = JSON.parse(
    execFileSync('/usr/bin/plutil', ['-convert', 'json', '-o', '-', join(contents, 'Info.plist')], {
      encoding: 'utf8',
    }),
  )
  assert.equal(plist.CFBundleIdentifier, config.identifier)
  assert.equal(plist.CFBundleShortVersionString, config.version)

  const executable = join(contents, 'MacOS', plist.CFBundleExecutable)
  assert.ok(statSync(executable).mode & 0o111, 'App executable lost its execute permission')
  parseLipoArchs(execFileSync('/usr/bin/lipo', ['-archs', executable], { encoding: 'utf8' }))
  execFileSync('/usr/bin/codesign', ['--verify', '--deep', '--strict', appPath], { stdio: 'pipe' })

  for (const name of files) {
    assert.ok(
      readFileSync(join(resources, name)).equals(readFileSync(join(rootDir, name))),
      `Packaged resource is missing or stale: ${name}`,
    )
  }

  verifyArtifactDirectory(contents)

  return {
    executable,
    executableSha256: createHash('sha256').update(readFileSync(executable)).digest('hex'),
  }
}

export function verifyMacosBundle(argv = process.argv.slice(2), rootDir = root) {
  const config = JSON.parse(readFileSync(join(rootDir, 'src-tauri/tauri.conf.json'), 'utf8'))
  const macosConfig = JSON.parse(readFileSync(join(rootDir, 'src-tauri/tauri.macos.conf.json'), 'utf8'))
  verifyMacosBundleConfig(config, macosConfig)

  const parsed = parseArgs(argv)
  const expectedArch = normalizeExpectedArch(parsed.expectedArch ?? expectedArchFromTarget(parsed.target))
  assert.ok(!expectedArch || expectedArch === 'arm64')

  const bundleDir = resolveBundleDir(parsed, rootDir)
  const { appPath, dmgPath } = verifyMacosBundleLayout(bundleDir, config.productName, config.version, {
    requireDmg: parsed.requireDmg,
  })
  const files = expectedResourceFiles(rootDir)
  const appInfo = verifyAppContents(appPath, config, files, rootDir)

  let dmgInfo = null
  if (dmgPath) {
    const dmgBytes = readFileSync(dmgPath)
    const dmgSha256 = createHash('sha256').update(dmgBytes).digest('hex')
    const mountDir = mkdtempSync(join(tmpdir(), 'sculpt-dmg-mount-'))
    try {
      execFileSync('/usr/bin/hdiutil', ['attach', '-readonly', '-nobrowse', '-mountpoint', mountDir, dmgPath], {
        stdio: 'pipe',
      })
      const mountedApp = join(mountDir, `${config.productName}.app`)
      assert.ok(existsSync(mountedApp), `Mounted DMG does not contain ${config.productName}.app`)
      verifyAppContents(mountedApp, config, files, rootDir)
    } finally {
      try {
        execFileSync('/usr/bin/hdiutil', ['detach', mountDir, '-force'], { stdio: 'pipe' })
      } catch {
        // Ignore if mount failed before attach succeeded
      }
      rmSync(mountDir, { recursive: true, force: true })
    }
    dmgInfo = {
      path: dmgPath,
      bytes: dmgBytes.length,
      sha256: dmgSha256,
      verifiedContents: true,
    }
  }

  const summary = {
    app: appPath,
    version: config.version,
    architecture: 'arm64',
    verifiedResources: files.length,
    executableSha256: appInfo.executableSha256,
    dmg: dmgInfo,
    signing: 'Local signature verified; this does not establish Developer ID notarization',
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
  verifyMacosBundle()
}
