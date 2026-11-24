import { spawnSync } from 'node:child_process'
import { join, resolve } from 'node:path'

const dir = import.meta.dirname
const args = process.argv.slice(2)

let script
if (process.platform === 'darwin') {
  script = join(dir, 'verify-macos-bundle.mjs')
} else if (process.platform === 'win32') {
  script = join(dir, 'verify-windows-bundle.mjs')
} else if (process.platform === 'linux') {
  script = join(dir, 'verify-linux-bundle.mjs')
} else {
  console.error(`No bundle verifier is configured for platform: ${process.platform}`)
  process.exit(1)
}

const result = spawnSync(process.execPath, [resolve(script), ...args], { stdio: 'inherit' })
process.exit(result.status ?? 1)
