// Verify the packaged app, not just the developer checkout. No model downloads.
import assert from 'node:assert/strict'
import { execFileSync } from 'node:child_process'
import { createHash } from 'node:crypto'
import { readFileSync, readdirSync, statSync } from 'node:fs'
import { resolve, join } from 'node:path'

const root = resolve(import.meta.dirname, '..')
const app = resolve(process.argv[2] ?? join(root, 'src-tauri/target/release/bundle/macos/Sculpt.app'))
const contents = join(app, 'Contents')
const resources = join(contents, 'Resources')
const plist = JSON.parse(execFileSync('/usr/bin/plutil', ['-convert', 'json', '-o', '-', join(contents, 'Info.plist')], { encoding: 'utf8' }))
const config = JSON.parse(readFileSync(join(root, 'src-tauri/tauri.conf.json'), 'utf8'))
assert.equal(plist.CFBundleIdentifier, config.identifier)
assert.equal(plist.CFBundleShortVersionString, config.version)
const executable = join(contents, 'MacOS', plist.CFBundleExecutable)
assert.ok(statSync(executable).mode & 0o111, 'App executable lost its execute permission')
assert.equal(execFileSync('/usr/bin/lipo', ['-archs', executable], { encoding: 'utf8' }).trim(), 'arm64')
execFileSync('/usr/bin/codesign', ['--verify', '--deep', '--strict', app], { stdio: 'pipe' })

const files = ['LICENSE', 'backend/TEXTURE-NOTICES.txt', 'backend/worker.py',
  'backend/setup_runtime.py', 'backend/download_models.py', 'backend/requirements.lock',
  'backend/runtime-spec.json', ...readdirSync(join(root, 'backend/sculpt_backend'))
    .filter(name => name.endsWith('.py')).map(name => `backend/sculpt_backend/${name}`)]
for (const name of files) {
  assert.ok(readFileSync(join(resources, name)).equals(readFileSync(join(root, name))),
    `Packaged resource is missing or stale: ${name}`)
}
console.log(JSON.stringify({ app, version: config.version, architecture: 'arm64',
  verifiedResources: files.length, executableSha256: createHash('sha256').update(readFileSync(executable)).digest('hex'),
  signing: 'Local signature verified; this does not establish Developer ID notarization' }, null, 2))
