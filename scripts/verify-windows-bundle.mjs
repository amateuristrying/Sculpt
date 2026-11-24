// Verify the built Windows desktop executable, NSIS installer, and packaged resources.
// No model downloads or inference claims.
import assert from 'node:assert/strict'
import { createHash } from 'node:crypto'
import { existsSync, readFileSync, readdirSync, statSync } from 'node:fs'
import { join, resolve } from 'node:path'

const root = resolve(import.meta.dirname, '..')
const config = JSON.parse(readFileSync(join(root, 'src-tauri/tauri.conf.json'), 'utf8'))

const arg = process.argv[2]
const defaultTargetDir = existsSync(join(root, 'src-tauri/target/x86_64-pc-windows-msvc/release/sculpt.exe'))
  ? join(root, 'src-tauri/target/x86_64-pc-windows-msvc/release')
  : join(root, 'src-tauri/target/release')

const releaseDir = resolve(arg ?? defaultTargetDir)
const exePath = releaseDir.endsWith('.exe') ? releaseDir : join(releaseDir, 'sculpt.exe')
const baseDir = releaseDir.endsWith('.exe') ? resolve(exePath, '..') : releaseDir

assert.ok(existsSync(exePath), `Built Windows executable not found: ${exePath}`)
const exeBytes = readFileSync(exePath)
assert.equal(exeBytes.subarray(0, 2).toString('ascii'), 'MZ', 'sculpt.exe is missing DOS MZ header')
const peOffset = exeBytes.readUInt32LE(0x3c)
assert.equal(exeBytes.subarray(peOffset, peOffset + 4).toString('ascii'), 'PE\0\0', 'sculpt.exe is missing PE signature')
const machine = exeBytes.readUInt16LE(peOffset + 4)
const archName = machine === 0x8664 ? 'x64' : machine === 0xaa64 ? 'arm64' : `unknown(0x${machine.toString(16)})`
assert.ok(machine === 0x8664 || machine === 0xaa64, `Unexpected PE machine architecture: ${archName}`)

const nsisDir = join(baseDir, 'bundle', 'nsis')
const expectedInstaller = join(nsisDir, `${config.productName}_${config.version}_${archName}-setup.exe`)
assert.ok(existsSync(expectedInstaller), `NSIS installer not found: ${expectedInstaller}`)
const installerStat = statSync(expectedInstaller)
assert.ok(installerStat.size > 0, `NSIS installer is empty: ${expectedInstaller}`)

const files = [
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

for (const name of files) {
  const packaged = join(baseDir, name)
  assert.ok(existsSync(packaged), `Packaged resource is missing next to executable: ${packaged}`)
  assert.ok(
    readFileSync(packaged).equals(readFileSync(join(root, name))),
    `Packaged resource is stale: ${name}`,
  )
}

const executableSha256 = createHash('sha256').update(exeBytes).digest('hex')
const installerSha256 = createHash('sha256').update(readFileSync(expectedInstaller)).digest('hex')

console.log(
  JSON.stringify(
    {
      executable: exePath,
      installer: expectedInstaller,
      installerBytes: installerStat.size,
      version: config.version,
      architecture: archName,
      verifiedResources: files.length,
      executableSha256,
      installerSha256,
      signing: 'Unsigned local/candidate build',
    },
    null,
    2,
  ),
)
