import { spawnSync } from 'node:child_process'
import { existsSync } from 'node:fs'
import { join, resolve } from 'node:path'

const root = resolve(import.meta.dirname, '..')
const [mode, ...rest] = process.argv.slice(2)

if (!mode || (mode !== 'system' && mode !== 'venv')) {
  console.error('Usage: node scripts/run-python.mjs <system|venv> [args...]')
  process.exit(2)
}

function resolveSystemPython() {
  if (process.env.SCULPT_PYTHON && existsSync(process.env.SCULPT_PYTHON)) {
    return { cmd: process.env.SCULPT_PYTHON, prefix: [] }
  }
  const candidates = process.platform === 'win32'
    ? [
        { cmd: 'python', prefix: [] },
        { cmd: 'py', prefix: ['-3'] },
        { cmd: 'python3', prefix: [] },
      ]
    : [
        { cmd: 'python3', prefix: [] },
        { cmd: 'python', prefix: [] },
      ]
  for (const candidate of candidates) {
    const check = spawnSync(candidate.cmd, [...candidate.prefix, '--version'], { stdio: 'ignore' })
    if (check.status === 0) return candidate
  }
  return null
}

function resolveVenvPython() {
  const runtimeDir = process.env.SCULPT_RUNTIME_DIR || join(root, '.sculpt-runtime')
  const candidates = process.platform === 'win32'
    ? [
        join(runtimeDir, 'venv', 'Scripts', 'python.exe'),
        join(runtimeDir, 'venv', 'python.exe'),
      ]
    : [join(runtimeDir, 'venv', 'bin', 'python')]
  for (const candidate of candidates) {
    if (existsSync(candidate)) return { cmd: candidate, prefix: [] }
  }
  return null
}

const target = mode === 'venv' ? resolveVenvPython() : resolveSystemPython()
if (!target) {
  console.error(
    mode === 'venv'
      ? 'Local Sculpt Python runtime not found. Run runtime setup first.'
      : 'Python 3 executable not found on PATH.',
  )
  process.exit(1)
}

const result = spawnSync(target.cmd, [...target.prefix, ...rest], {
  cwd: root,
  stdio: 'inherit',
  env: process.env,
})
process.exit(result.status ?? 1)
