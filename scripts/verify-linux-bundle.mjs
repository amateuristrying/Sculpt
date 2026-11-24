// Verify the built Linux desktop executable, .deb package metadata/contents,
// ELF binary architecture, WebKitGTK 4.1 runtime dependencies, and packaged resources.
// Ubuntu 22.04 x64 (.deb only) is the sole initial Linux target. No model downloads or inference claims.
import assert from 'node:assert/strict'
import { execFileSync } from 'node:child_process'
import { createHash } from 'node:crypto'
import { existsSync, openSync, readFileSync, readSync, readdirSync, realpathSync, statSync, closeSync } from 'node:fs'
import { join, relative, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { verifyArtifactDirectory } from './verify-ci-hygiene.mjs'

const root = resolve(import.meta.dirname, '..')
const CARGO_INTERNAL_DIRS = new Set([
  'deps',
  'build',
  '.fingerprint',
  'incremental',
  'examples',
  'bundle',
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
  if (target.startsWith('x86_64-')) return 'x64'
  if (target.startsWith('aarch64-')) return 'arm64'
  return null
}

export function normalizeExpectedArch(arch) {
  if (!arch) return null
  const lower = arch.toLowerCase()
  if (lower === 'amd64' || lower === 'x86_64' || lower === 'x64') return 'x64'
  if (lower === 'aarch64' || lower === 'arm64') return 'arm64'
  return lower
}

export function expectedDebArch(arch) {
  if (arch === 'x64') return 'amd64'
  if (arch === 'arm64') return 'arm64'
  throw new Error(`Unsupported Linux architecture for .deb packaging: ${arch}`)
}

export function inspectElfBuffer(bytes, filePath = '<buffer>') {
  assert.ok(bytes.length >= 20, `ELF file is truncated: ${filePath}`)
  assert.equal(bytes[0], 0x7f, `${filePath} is missing ELF magic`)
  assert.equal(bytes.subarray(1, 4).toString('ascii'), 'ELF', `${filePath} is missing ELF magic`)
  const elfClass = bytes[4]
  assert.equal(elfClass, 2, `Expected 64-bit ELF binary (class 2), found class ${elfClass} in ${filePath}`)
  const endian = bytes[5]
  assert.equal(endian, 1, `Expected little-endian ELF binary, found endianness ${endian} in ${filePath}`)
  const machine = bytes.readUInt16LE(18)
  const machineHex = `0x${machine.toString(16)}`
  const architecture =
    machine === 0x3e
      ? 'x64'
      : machine === 0xb7
        ? 'arm64'
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

function hasElfMagic(filePath) {
  let fd
  try {
    fd = openSync(filePath, 'r')
    const header = Buffer.alloc(4)
    const read = readSync(fd, header, 0, 4, 0)
    return read === 4 && header[0] === 0x7f && header[1] === 0x45 && header[2] === 0x4c && header[3] === 0x46
  } catch {
    return false
  } finally {
    if (fd !== undefined) closeSync(fd)
  }
}

export function collectPackagedElfBinaries(baseDir) {
  const found = []
  function walk(dir, isRoot) {
    for (const entry of readdirSync(dir, { withFileTypes: true })) {
      if (isRoot && entry.isDirectory() && CARGO_INTERNAL_DIRS.has(entry.name)) {
        continue
      }
      const fullPath = join(dir, entry.name)
      if (entry.isDirectory()) {
        walk(fullPath, false)
      } else if (entry.isFile() && hasElfMagic(fullPath)) {
        found.push(fullPath)
      }
    }
  }
  walk(baseDir, true)
  return found.sort()
}

export function parseDebControlMetadata(controlOutput) {
  const fields = {}
  let currentKey = null
  for (const line of controlOutput.split(/\r?\n/)) {
    if (!line.trim()) continue
    if (/^\s+/.test(line) && currentKey) {
      fields[currentKey] = `${fields[currentKey]}\n${line.trim()}`
      continue
    }
    const idx = line.indexOf(':')
    if (idx === -1) continue
    const key = line.slice(0, idx).trim().toLowerCase()
    const value = line.slice(idx + 1).trim()
    fields[key] = value
    currentKey = key
  }
  return fields
}

export function parseDebDependsList(dependsValue = '') {
  return dependsValue
    .split(',')
    .map(part => part.trim())
    .filter(Boolean)
    .map(part => part.split(/[\s(|]/)[0].trim())
    .filter(Boolean)
}

export function verifyDebControlMetadata(fields, { expectedVersion, expectedDebArchitecture }) {
  assert.ok(fields.package, 'Debian control metadata is missing Package field')
  assert.equal(
    fields.version,
    expectedVersion,
    `Expected .deb Version ${expectedVersion}, found ${fields.version}`,
  )
  assert.equal(
    fields.architecture,
    expectedDebArchitecture,
    `Expected .deb Architecture ${expectedDebArchitecture}, found ${fields.architecture}`,
  )
  const deps = parseDebDependsList(fields.depends)
  assert.ok(
    deps.includes('libwebkit2gtk-4.1-0'),
    `Expected .deb Depends to include libwebkit2gtk-4.1-0; found: ${fields.depends ?? '(none)'}`,
  )
  assert.ok(
    deps.includes('libgtk-3-0'),
    `Expected .deb Depends to include libgtk-3-0; found: ${fields.depends ?? '(none)'}`,
  )
  assert.ok(
    !deps.includes('libwebkit2gtk-4.0-37'),
    'Unexpected legacy WebKitGTK 4.0 dependency found; Tauri 2 requires WebKitGTK 4.1',
  )
  return {
    package: fields.package,
    version: fields.version,
    architecture: fields.architecture,
    section: fields.section ?? null,
    depends: deps,
  }
}

export function verifyDebFileList(listingOutput, expectedResources) {
  const entries = listingOutput
    .split(/\r?\n/)
    .map(line => line.trim())
    .filter(Boolean)
    .map(line => {
      const match = line.match(/^(\S+)\s+\S+\s+\S+\s+\S+\s+\S+\s+(\S+)(?:\s+->\s+.*)?$/)
      if (!match) return null
      const [, mode, rawPath] = match
      const normalizedPath = `/${rawPath.replace(/^\.\//, '')}`
      return { mode, path: normalizedPath }
    })
    .filter(Boolean)

  const binaryEntry = entries.find(e => e.path === '/usr/bin/sculpt')
  assert.ok(binaryEntry, 'Expected /usr/bin/sculpt inside .deb archive')
  assert.ok(binaryEntry.mode.includes('x'), `Expected executable mode on /usr/bin/sculpt, found ${binaryEntry.mode}`)

  const desktopEntry = entries.find(
    e => e.path.startsWith('/usr/share/applications/') && e.path.endsWith('.desktop'),
  )
  assert.ok(desktopEntry, 'Expected .desktop entry under /usr/share/applications/ inside .deb archive')

  const iconEntries = entries.filter(
    e => e.path.startsWith('/usr/share/icons/hicolor/') && e.path.endsWith('.png'),
  )
  assert.ok(iconEntries.length > 0, 'Expected PNG hicolor icon(s) under /usr/share/icons/hicolor/ inside .deb archive')

  const missingResources = []
  for (const res of expectedResources) {
    const found = entries.some(
      e => e.path.startsWith('/usr/lib/') && e.path.endsWith(`/${res}`),
    )
    if (!found) missingResources.push(res)
  }
  assert.equal(
    missingResources.length,
    0,
    `Missing bundled resource(s) inside .deb archive: ${missingResources.join(', ')}`,
  )

  return {
    binary: binaryEntry.path,
    desktopFile: desktopEntry.path,
    iconCount: iconEntries.length,
    verifiedDebResources: expectedResources.length,
  }
}

function resolveReleaseDir({ target, positionalPath }) {
  if (positionalPath) {
    return resolve(positionalPath)
  }
  if (target) {
    return join(root, 'src-tauri', 'target', target, 'release')
  }
  const hostTriple = process.arch === 'arm64' ? 'aarch64-unknown-linux-gnu' : 'x86_64-unknown-linux-gnu'
  const candidates = [
    join(root, 'src-tauri', 'target', hostTriple, 'release'),
    join(root, 'src-tauri', 'target', 'release'),
  ]
  return candidates.find(dir => existsSync(join(dir, 'sculpt'))) ?? candidates[0]
}

export function expectedResourceFiles() {
  return [
    'LICENSE',
    'backend/TEXTURE-NOTICES.txt',
    'backend/worker.py',
    'backend/setup_runtime.py',
    'backend/download_models.py',
    'backend/requirements.lock',
    'backend/runtime-spec.json',
    ...readdirSync(join(root, 'backend/sculpt_backend'))
      .filter(name => name.endsWith('.py'))
      .map(name => `backend/sculpt_backend/${name}`),
  ]
}

export function verifyLinuxBundle(argv = process.argv.slice(2)) {
  const config = JSON.parse(readFileSync(join(root, 'src-tauri/tauri.conf.json'), 'utf8'))
  const linuxConfig = JSON.parse(readFileSync(join(root, 'src-tauri/tauri.linux.conf.json'), 'utf8'))
  assert.deepEqual(
    linuxConfig.bundle?.targets,
    ['deb'],
    'tauri.linux.conf.json must target only ["deb"] for Ubuntu 22.04',
  )

  const parsed = parseArgs(argv)
  const expectedArch = normalizeExpectedArch(parsed.expectedArch ?? expectedArchFromTarget(parsed.target))
  const releaseDir = resolveReleaseDir(parsed)
  const exePath = releaseDir.endsWith('.deb')
    ? resolve(releaseDir, '..', '..', '..', 'sculpt')
    : join(releaseDir, 'sculpt')
  const baseDir = resolve(exePath, '..')

  assert.ok(existsSync(exePath), `Built Linux executable not found: ${exePath}`)
  if (process.platform === 'linux') {
    assert.ok(statSync(exePath).mode & 0o111, `Linux executable lost execute permission: ${exePath}`)
  }

  const exeInfo = inspectElfBuffer(readFileSync(exePath), exePath)
  assert.ok(
    exeInfo.architecture === 'x64' || exeInfo.architecture === 'arm64',
    `Unexpected ELF architecture for sculpt: ${exeInfo.architecture} (${exeInfo.machineHex})`,
  )
  if (expectedArch) {
    assert.equal(
      exeInfo.architecture,
      expectedArch,
      `Expected ${expectedArch} sculpt binary, found ${exeInfo.architecture} (${exeInfo.machineHex}) at ${exePath}`,
    )
  }

  const packagedBinaries = collectPackagedElfBinaries(baseDir).map(binPath => {
    const info = inspectElfBuffer(readFileSync(binPath), binPath)
    return {
      relativePath: relative(baseDir, binPath).replaceAll('\\', '/'),
      architecture: info.architecture,
      machineHex: info.machineHex,
      bytes: info.bytes,
      sha256: info.sha256,
    }
  })

  const foreignArchHelpers = packagedBinaries.filter(bin => bin.architecture !== exeInfo.architecture)
  assert.equal(
    foreignArchHelpers.length,
    0,
    `Packaged payload contains foreign-architecture ELF binaries (${foreignArchHelpers.map(b => `${b.relativePath}:${b.architecture}`).join(', ')})`,
  )

  const files = expectedResourceFiles()
  for (const name of files) {
    const packaged = join(baseDir, name)
    assert.ok(existsSync(packaged), `Packaged resource is missing next to executable: ${packaged}`)
    assert.ok(
      readFileSync(packaged).equals(readFileSync(join(root, name))),
      `Packaged resource is stale: ${name}`,
    )
  }

  const bundleDir = join(baseDir, 'bundle')
  assert.ok(!existsSync(join(bundleDir, 'appimage')), 'Unexpected AppImage bundle directory; only .deb is supported')
  assert.ok(!existsSync(join(bundleDir, 'rpm')), 'Unexpected RPM bundle directory; only .deb is supported')

  const debArch = expectedDebArch(exeInfo.architecture)
  const debDir = join(bundleDir, 'deb')
  assert.ok(existsSync(debDir), `Debian bundle directory not found: ${debDir}`)
  const expectedSuffix = `_${config.version}_${debArch}.deb`.toLowerCase()
  const debName = readdirSync(debDir).find(
    name => name.toLowerCase().startsWith(`${config.productName.toLowerCase()}_`) && name.toLowerCase().endsWith(expectedSuffix),
  )
  assert.ok(debName, `Expected .deb package ending in ${expectedSuffix} inside ${debDir}`)

  const debPath = join(debDir, debName)
  const debBytes = readFileSync(debPath)
  assert.ok(debBytes.length > 0, `.deb package is empty: ${debPath}`)
  const debSha256 = createHash('sha256').update(debBytes).digest('hex')

  const controlRaw = execFileSync('dpkg-deb', ['-f', debPath], { encoding: 'utf8' })
  const controlSummary = verifyDebControlMetadata(parseDebControlMetadata(controlRaw), {
    expectedVersion: config.version,
    expectedDebArchitecture: debArch,
  })
  const contentsRaw = execFileSync('dpkg-deb', ['-c', debPath], { encoding: 'utf8' })
  const contentsSummary = verifyDebFileList(contentsRaw, files)
  verifyArtifactDirectory(baseDir)

  const summary = {
    executable: exeInfo.path,
    version: config.version,
    architecture: exeInfo.architecture,
    debArchitecture: debArch,
    machineHex: exeInfo.machineHex,
    executableSha256: exeInfo.sha256,
    debPackage: debPath,
    debBytes: debBytes.length,
    debSha256,
    debControl: controlSummary,
    debContents: contentsSummary,
    packagedBinaries,
    foreignArchHelpers,
    verifiedResources: files.length,
    signing: 'Unsigned local/candidate .deb package',
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
  verifyLinuxBundle()
}
