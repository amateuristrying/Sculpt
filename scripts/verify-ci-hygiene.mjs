// Verify CI workflow safety, repository privacy hygiene, ignore rules, Tauri bundle
// resource exclusions, and candidate build artifact hygiene (no model weights, user data,
// or private planning material). No network calls or release publishing.
import assert from 'node:assert/strict'
import { createHash } from 'node:crypto'
import { existsSync, mkdirSync, readFileSync, readdirSync, realpathSync, statSync, writeFileSync } from 'node:fs'
import { dirname, extname, join, relative, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const root = resolve(import.meta.dirname, '..')
const PRIVATE_DIR = ['.', 'agents'].join('')
const PRIVATE_DIR_REGEX = new RegExp(`(?:^|[^a-zA-Z0-9_-])\\.${'agents'}(?:$|[^a-zA-Z0-9_-])`)

const CARGO_INTERNAL_DIRS = new Set([
  'deps',
  'build',
  '.fingerprint',
  'incremental',
  'examples',
])

const FORBIDDEN_ARTIFACT_SEGMENTS = new Set([
  PRIVATE_DIR,
  '.env',
  '.sculpt-runtime',
  '.sculpt-library',
  '.sculpt-models',
  '.sculpt-cache',
  'library-development',
  '__pycache__',
  'library.json',
  '.library.lock',
  'sculpt.log',
  'sculpt.prev.log',
])

const FORBIDDEN_ARTIFACT_EXTENSIONS = new Set([
  '.pyc',
  '.pyo',
  '.safetensors',
  '.ckpt',
  '.pth',
  '.onnx',
  '.gguf',
])

const FORBIDDEN_WORKFLOW_PATTERNS = [
  { label: 'gh release command', regex: /\bgh\s+release\b/i },
  { label: 'softprops/action-gh-release', regex: /softprops\/action-gh-release/i },
  { label: 'actions/create-release', regex: /actions\/create-release/i },
  { label: 'actions/upload-release-asset', regex: /actions\/upload-release-asset/i },
  { label: 'ncipollo/release-action', regex: /ncipollo\/release-action/i },
  { label: 'tauri-apps/tauri-action', regex: /tauri-apps\/tauri-action/i },
  { label: 'docker push', regex: /\bdocker\s+push\b/i },
  { label: 'docker/build-push-action', regex: /docker\/build-push-action/i },
  { label: 'wrangler deploy', regex: /\bwrangler\s+deploy\b/i },
  { label: 'vercel deploy', regex: /\bvercel(?:\s+deploy|\s+--prod)?\b/i },
  { label: 'netlify deploy', regex: /\bnetlify\s+deploy\b/i },
  { label: 'gh-pages deploy', regex: /gh-pages/i },
  { label: 'npm publish', regex: /\bnpm\s+publish\b/i },
  { label: 'cargo publish', regex: /\bcargo\s+publish\b/i },
  { label: 'twine upload', regex: /\btwine\s+upload\b/i },
]

const EXPECTED_JOBS = {
  'static-hygiene': { runsOn: 'ubuntu-22.04', isBuildLane: false },
  'windows-x64': {
    runsOn: 'windows-latest',
    isBuildLane: true,
    artifactName: 'Sculpt-Windows-x64-shell',
    verifyCommand: 'npm run desktop:verify:windows-x64',
    requiredBuildFlags: ['--target x86_64-pc-windows-msvc', '--bundles nsis'],
    forbiddenPatterns: [/\b--bundles\s+[^\n]*msi\b/i, /\bx86_64-unknown-linux\b/i],
  },
  'windows-arm64': {
    runsOn: 'windows-11-arm',
    isBuildLane: true,
    artifactName: 'Sculpt-Windows-arm64-shell',
    verifyCommand: 'npm run desktop:verify:windows-arm64',
    requiredBuildFlags: ['--target aarch64-pc-windows-msvc', '--bundles nsis'],
    forbiddenPatterns: [/\b--bundles\s+[^\n]*msi\b/i],
  },
  'ubuntu-x64': {
    runsOn: 'ubuntu-22.04',
    isBuildLane: true,
    artifactName: 'Sculpt-Ubuntu-22.04-x64-shell',
    verifyCommand: 'npm run desktop:verify:linux-x64',
    requiredBuildFlags: ['--target x86_64-unknown-linux-gnu', '--bundles deb'],
    forbiddenPatterns: [/\b(appimage|rpm|snap|flatpak)\b/i],
  },
  'macos-arm64': {
    runsOn: 'macos-15',
    isBuildLane: true,
    artifactName: 'Sculpt-macOS-arm64-preview',
    verifyCommand: 'npm run desktop:verify:macos-arm64',
    requiredBuildFlags: ['--target aarch64-apple-darwin', '--bundles app,dmg'],
    forbiddenPatterns: [/\bx86_64-apple-darwin\b/i, /\buniversal-apple-darwin\b/i, /\b--bundles\s+[^\n]*pkg\b/i, /Sculpt-macOS-arm64\.zip/i],
  },
}

const PUBLIC_SCAN_TARGETS = [
  'README.md',
  'LICENSE',
  'index.html',
  'package.json',
  '.env.example',
  'playwright.config.ts',
  'tsconfig.json',
  'vite.config.ts',
  'vite.config.local.ts',
  'vitest.config.ts',
  'docs',
  'src',
  'src-tauri/src',
  'src-tauri/Cargo.toml',
  'src-tauri/tauri.conf.json',
  'src-tauri/tauri.windows.conf.json',
  'src-tauri/tauri.linux.conf.json',
  'src-tauri/tauri.macos.conf.json',
  'src-tauri/capabilities',
  'scripts',
  '.github',
  'tests',
  'backend',
]

const SKIP_SCAN_DIRS = new Set(['node_modules', 'target', '__pycache__', '.pytest_cache', '.venv', '.git'])

export function parseArgs(argv) {
  let checkRepo = false
  let checkArtifact = null
  let artifactGlob = null
  let writeChecksums = null

  for (let i = 0; i < argv.length; i += 1) {
    const token = argv[i]
    if (token === '--check-repo') {
      checkRepo = true
    } else if (token === '--check-artifact') {
      checkArtifact = argv[++i] ?? null
    } else if (token.startsWith('--check-artifact=')) {
      checkArtifact = token.slice('--check-artifact='.length)
    } else if (token === '--artifact-glob') {
      artifactGlob = argv[++i] ?? null
    } else if (token.startsWith('--artifact-glob=')) {
      artifactGlob = token.slice('--artifact-glob='.length)
    } else if (token === '--write-checksums') {
      writeChecksums = argv[++i] ?? null
    } else if (token.startsWith('--write-checksums=')) {
      writeChecksums = token.slice('--write-checksums='.length)
    } else {
      throw new Error(`Unknown option: ${token}`)
    }
  }

  if (!checkRepo && !checkArtifact) {
    checkRepo = true
  }

  return { checkRepo, checkArtifact, artifactGlob, writeChecksums }
}

export function validateGitignore(gitignoreText) {
  const lines = gitignoreText
    .split(/\r?\n/)
    .map(line => line.trim())
    .filter(line => line.length > 0 && !line.startsWith('#'))

  const requiredEntries = [
    `${PRIVATE_DIR}/`,
    '.env',
    '.sculpt-runtime/',
    '.sculpt-library/',
    '.sculpt-models/',
    '.sculpt-cache/',
  ]

  const missing = requiredEntries.filter(
    entry => !lines.some(line => line === entry || line === entry.replace(/\/$/, '')),
  )
  assert.equal(missing.length, 0, `.gitignore is missing required exclusion(s): ${missing.join(', ')}`)
  return requiredEntries
}

export function scanPublicFilesForLeaks(rootDir = root) {
  const findings = []

  function walk(targetPath) {
    if (!existsSync(targetPath)) return
    const stat = statSync(targetPath)
    if (stat.isDirectory()) {
      for (const entry of readdirSync(targetPath, { withFileTypes: true })) {
        if (SKIP_SCAN_DIRS.has(entry.name)) continue
        walk(join(targetPath, entry.name))
      }
      return
    }
    const ext = extname(targetPath).toLowerCase()
    if (['.png', '.jpg', '.jpeg', '.webp', '.ico', '.icns', '.glb', '.stl', '.pyc'].includes(ext)) {
      return
    }
    const content = readFileSync(targetPath, 'utf8')
    const lines = content.split(/\r?\n/)
    lines.forEach((line, idx) => {
      if (PRIVATE_DIR_REGEX.test(line)) {
        findings.push({
          file: relative(rootDir, targetPath).replaceAll('\\', '/'),
          line: idx + 1,
        })
      }
    })
  }

  for (const target of PUBLIC_SCAN_TARGETS) {
    walk(join(rootDir, target))
  }

  return findings
}

export function parseWorkflowJobs(workflowText) {
  const lines = workflowText.split(/\r?\n/)
  const jobsIndex = lines.findIndex(line => /^jobs:\s*$/.test(line))
  assert.ok(jobsIndex !== -1, 'Workflow must define a top-level jobs: mapping')

  const topLevel = lines.slice(0, jobsIndex).join('\n')
  const jobs = {}
  let currentJobId = null
  let currentLines = []

  for (let i = jobsIndex + 1; i < lines.length; i += 1) {
    const line = lines[i]
    const jobMatch = line.match(/^  ([a-zA-Z0-9_-]+):\s*(?:#.*)?$/)
    if (jobMatch) {
      if (currentJobId) {
        jobs[currentJobId] = currentLines.join('\n')
      }
      currentJobId = jobMatch[1]
      currentLines = []
    } else if (currentJobId) {
      currentLines.push(line)
    }
  }
  if (currentJobId) {
    jobs[currentJobId] = currentLines.join('\n')
  }

  return { topLevel, jobs }
}

export function validateCiWorkflow(workflowText) {
  assert.ok(!PRIVATE_DIR_REGEX.test(workflowText), 'Workflow must never reference private planning directory')

  for (const forbidden of FORBIDDEN_WORKFLOW_PATTERNS) {
    assert.ok(
      !forbidden.regex.test(workflowText),
      `Workflow contains forbidden release or deploy pattern (${forbidden.label})`,
    )
  }

  assert.ok(
    !/^\s*(contents|packages|id-token|actions|deployments)\s*:\s*write\b/m.test(workflowText) &&
      !/^\s*permissions\s*:\s*write-all\b/m.test(workflowText),
    'Workflow must not request write permissions',
  )

  const { topLevel, jobs } = parseWorkflowJobs(workflowText)

  assert.match(topLevel, /^permissions:\s*\n\s+contents:\s*read\s*$/m, 'Workflow must declare permissions: contents: read')
  assert.match(topLevel, /^concurrency:\s*$/m, 'Workflow must declare top-level concurrency')
  assert.match(topLevel, /^\s+group:\s*\S+/m, 'Workflow concurrency must specify group')
  assert.match(topLevel, /^\s+cancel-in-progress:\s*\S+/m, 'Workflow concurrency must specify cancel-in-progress')
  assert.match(topLevel, /^\s+HF_HUB_OFFLINE:\s*['"]?1['"]?\s*$/m, 'Workflow env must set HF_HUB_OFFLINE: 1')
  assert.match(topLevel, /^\s+TRANSFORMERS_OFFLINE:\s*['"]?1['"]?\s*$/m, 'Workflow env must set TRANSFORMERS_OFFLINE: 1')

  const uploadBlocks = workflowText.match(/- uses:\s*actions\/upload-artifact@v4[\s\S]*?(?=\n\s+- |\n  [a-zA-Z0-9_-]+:|\s*$)/g) ?? []
  assert.ok(uploadBlocks.length >= 4, 'Expected candidate artifact upload steps for all four platform lanes')
  for (const block of uploadBlocks) {
    const retentionMatch = block.match(/retention-days:\s*(\d+)/)
    assert.ok(retentionMatch, `upload-artifact step missing retention-days:\n${block}`)
    const days = Number(retentionMatch[1])
    assert.ok(days >= 1 && days <= 30, `Artifact retention-days must be between 1 and 30, got ${days}`)
    assert.match(block, /if-no-files-found:\s*(error|ignore)/, `upload-artifact step missing if-no-files-found:\n${block}`)
    for (const forbiddenSegment of FORBIDDEN_ARTIFACT_SEGMENTS) {
      assert.ok(
        !block.includes(forbiddenSegment),
        `upload-artifact step references forbidden path (${forbiddenSegment})`,
      )
    }
  }

  const jobSummaries = {}
  for (const [jobId, spec] of Object.entries(EXPECTED_JOBS)) {
    const body = jobs[jobId]
    assert.ok(body, `Missing expected CI job: ${jobId}`)

    const runsOnMatch = body.match(/^\s+runs-on:\s*([^\s#]+)/m)
    assert.ok(runsOnMatch, `Job ${jobId} missing runs-on`)
    assert.equal(runsOnMatch[1], spec.runsOn, `Job ${jobId} expected runs-on ${spec.runsOn}, got ${runsOnMatch[1]}`)

    const timeoutMatch = body.match(/^\s+timeout-minutes:\s*(\d+)/m)
    assert.ok(timeoutMatch, `Job ${jobId} missing timeout-minutes`)
    const timeout = Number(timeoutMatch[1])
    assert.ok(timeout > 0 && timeout <= 60, `Job ${jobId} timeout-minutes must be 1..60, got ${timeout}`)

    assert.ok(!/^\s+permissions:/m.test(body), `Job ${jobId} must not override top-level read-only permissions`)
    assert.ok(body.includes('npm run ci:hygiene'), `Job ${jobId} must run npm run ci:hygiene`)
    assert.ok(body.includes('npm test'), `Job ${jobId} must run npm test`)
    assert.ok(body.includes('npm run build'), `Job ${jobId} must run npm run build`)

    if (spec.isBuildLane) {
      assert.ok(body.includes('cargo +stable fmt --check'), `Job ${jobId} must run cargo +stable fmt --check`)
      assert.ok(body.includes('cargo +stable clippy'), `Job ${jobId} must run cargo +stable clippy`)
      assert.ok(body.includes('cargo +stable check --locked'), `Job ${jobId} must run cargo +stable check --locked`)
      assert.ok(body.includes('cargo +stable test --locked'), `Job ${jobId} must run cargo +stable test --locked`)
      assert.ok(body.includes('compileall') || body.includes('py_compile'), `Job ${jobId} must compile-check Python scripts`)
      assert.ok(body.includes('pytest'), `Job ${jobId} must run pytest`)
      assert.ok(body.includes('npm run desktop:build'), `Job ${jobId} must run npm run desktop:build`)
      assert.ok(body.includes(spec.verifyCommand), `Job ${jobId} must run ${spec.verifyCommand}`)
      for (const flag of spec.requiredBuildFlags ?? []) {
        assert.ok(body.includes(flag), `Job ${jobId} must include package matrix flag "${flag}"`)
      }
      for (const forbiddenPattern of spec.forbiddenPatterns ?? []) {
        assert.ok(
          !forbiddenPattern.test(body),
          `Job ${jobId} includes deferred/forbidden package target (${forbiddenPattern})`,
        )
      }
      assert.ok(body.includes('--check-artifact'), `Job ${jobId} must run artifact hygiene check before upload`)
      assert.ok(body.includes(`name: ${spec.artifactName}`), `Job ${jobId} must upload artifact ${spec.artifactName}`)
      assert.ok(body.includes('SHA256SUMS.txt'), `Job ${jobId} must generate and upload SHA256SUMS.txt`)
    }

    jobSummaries[jobId] = {
      runsOn: runsOnMatch[1],
      timeoutMinutes: timeout,
      artifactName: spec.artifactName ?? null,
    }
  }

  return {
    jobs: jobSummaries,
    uploadStepCount: uploadBlocks.length,
  }
}

export function validateTauriBundleConfigs(rootDir = root) {
  const baseConfig = JSON.parse(readFileSync(join(rootDir, 'src-tauri/tauri.conf.json'), 'utf8'))
  const windowsConfig = JSON.parse(readFileSync(join(rootDir, 'src-tauri/tauri.windows.conf.json'), 'utf8'))
  const linuxConfig = JSON.parse(readFileSync(join(rootDir, 'src-tauri/tauri.linux.conf.json'), 'utf8'))
  const macosConfig = JSON.parse(readFileSync(join(rootDir, 'src-tauri/tauri.macos.conf.json'), 'utf8'))

  const resources = Object.keys(baseConfig.bundle?.resources ?? {})
  assert.ok(resources.length > 0, 'tauri.conf.json must define explicit bundle.resources')
  for (const sourcePath of resources) {
    for (const forbidden of [PRIVATE_DIR, '.env', '.sculpt-runtime', '.sculpt-library', '.sculpt-models', '.sculpt-cache']) {
      assert.ok(!sourcePath.includes(forbidden), `bundle.resources must not include ${forbidden}: ${sourcePath}`)
    }
  }

  assert.deepEqual(baseConfig.bundle?.targets, ['app', 'dmg'], 'Base tauri.conf.json bundle.targets must remain ["app", "dmg"]')
  assert.deepEqual(windowsConfig.bundle?.targets, ['nsis'], 'Windows bundle targets must remain ["nsis"]')
  assert.equal(
    windowsConfig.bundle?.windows?.nsis?.installMode,
    'currentUser',
    'Windows NSIS installMode must remain "currentUser"',
  )
  assert.deepEqual(linuxConfig.bundle?.targets, ['deb'], 'Linux bundle targets must remain ["deb"]')
  assert.deepEqual(macosConfig.bundle?.targets, ['app', 'dmg'], 'macOS bundle targets must remain ["app", "dmg"]')
  assert.equal(
    macosConfig.bundle?.macOS?.minimumSystemVersion,
    '12.0',
    'macOS minimumSystemVersion must remain "12.0"',
  )

  return {
    resourceCount: resources.length,
    windowsTargets: windowsConfig.bundle.targets,
    windowsInstallMode: windowsConfig.bundle.windows.nsis.installMode,
    linuxTargets: linuxConfig.bundle.targets,
    macosTargets: macosConfig.bundle.targets,
  }
}

function globToRegExp(globPattern) {
  const normalized = globPattern.replaceAll('\\', '/')
  const escaped = normalized
    .replace(/[.+^${}()|[\]\\]/g, '\\$&')
    .replace(/\*/g, '[^/]*')
  return new RegExp(`^${escaped}$`)
}

export function isForbiddenArtifactEntry(relativePath) {
  const normalized = relativePath.replaceAll('\\', '/')
  const segments = normalized.split('/')
  for (const segment of segments) {
    const lower = segment.toLowerCase()
    if (lower === '.env.example') {
      continue
    }
    if (FORBIDDEN_ARTIFACT_SEGMENTS.has(lower)) {
      return `forbidden segment "${segment}"`
    }
    if (lower.startsWith('.env.') || lower.endsWith('.env')) {
      return `forbidden env file "${segment}"`
    }
  }
  const ext = extname(normalized).toLowerCase()
  if (FORBIDDEN_ARTIFACT_EXTENSIONS.has(ext)) {
    return `forbidden file extension "${ext}"`
  }
  return null
}

export function verifyArtifactDirectory(releaseDir, { artifactGlob = null, writeChecksums = null } = {}) {
  const resolvedDir = resolve(releaseDir)
  assert.ok(existsSync(resolvedDir), `Release directory does not exist: ${resolvedDir}`)

  const forbiddenHits = []
  const matchedArtifacts = []
  const globRegex = artifactGlob ? globToRegExp(artifactGlob) : null

  function walk(dir, isRoot) {
    for (const entry of readdirSync(dir, { withFileTypes: true })) {
      if (isRoot && entry.isDirectory() && CARGO_INTERNAL_DIRS.has(entry.name)) {
        continue
      }
      const fullPath = join(dir, entry.name)
      const rel = relative(resolvedDir, fullPath).replaceAll('\\', '/')
      const reason = isForbiddenArtifactEntry(rel)
      if (reason) {
        forbiddenHits.push({ path: rel, reason })
      }
      if (entry.isDirectory()) {
        walk(fullPath, false)
      } else if (entry.isFile() && globRegex && globRegex.test(rel)) {
        const bytes = readFileSync(fullPath)
        assert.ok(bytes.length > 0, `Candidate artifact is empty: ${fullPath}`)
        matchedArtifacts.push({
          relativePath: rel,
          fileName: entry.name,
          fullPath,
          bytes: bytes.length,
          sha256: createHash('sha256').update(bytes).digest('hex'),
        })
      }
    }
  }

  walk(resolvedDir, true)

  assert.equal(
    forbiddenHits.length,
    0,
    `Candidate build directory contains forbidden files/directories: ${forbiddenHits.map(h => `${h.path} (${h.reason})`).join(', ')}`,
  )

  if (globRegex) {
    assert.ok(
      matchedArtifacts.length > 0,
      `No candidate artifact matched "${artifactGlob}" under ${resolvedDir}`,
    )
  }

  if (writeChecksums) {
    assert.ok(matchedArtifacts.length > 0, 'Cannot write SHA256SUMS.txt without matched artifacts')
    const checksumPath = resolve(writeChecksums)
    mkdirSync(dirname(checksumPath), { recursive: true })
    const lines = matchedArtifacts
      .sort((a, b) => a.fileName.localeCompare(b.fileName))
      .map(item => `${item.sha256}  ${item.fileName}`)
      .join('\n')
    writeFileSync(checksumPath, `${lines}\n`, 'utf8')
  }

  return {
    releaseDir: resolvedDir,
    forbiddenHits,
    matchedArtifacts: matchedArtifacts.map(({ relativePath, fileName, bytes, sha256 }) => ({
      relativePath,
      fileName,
      bytes,
      sha256,
    })),
    checksumManifest: writeChecksums ? resolve(writeChecksums) : null,
  }
}

export function runCiHygiene(argv = process.argv.slice(2), rootDir = root) {
  const parsed = parseArgs(argv)
  const report = {}

  if (parsed.checkRepo) {
    const gitignore = readFileSync(join(rootDir, '.gitignore'), 'utf8')
    const requiredIgnores = validateGitignore(gitignore)
    const leaks = scanPublicFilesForLeaks(rootDir)
    assert.equal(
      leaks.length,
      0,
      `Private planning reference leaked into public file(s): ${leaks.map(l => `${l.file}:${l.line}`).join(', ')}`,
    )

    const workflowDir = join(rootDir, '.github/workflows')
    const workflowFiles = readdirSync(workflowDir).filter(name => name.endsWith('.yml') || name.endsWith('.yaml'))
    assert.deepEqual(workflowFiles, ['ci.yml'], `Unexpected workflow files: ${workflowFiles.join(', ')}`)
    const workflowText = readFileSync(join(workflowDir, 'ci.yml'), 'utf8')
    const workflowSummary = validateCiWorkflow(workflowText)
    const tauriSummary = validateTauriBundleConfigs(rootDir)

    report.repoHygiene = {
      requiredIgnores,
      publicLeaks: leaks.length,
      workflow: workflowSummary,
      tauriBundles: tauriSummary,
    }
  }

  if (parsed.checkArtifact) {
    report.artifactHygiene = verifyArtifactDirectory(parsed.checkArtifact, {
      artifactGlob: parsed.artifactGlob,
      writeChecksums: parsed.writeChecksums,
    })
  }

  console.log(JSON.stringify(report, null, 2))
  return report
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
  runCiHygiene()
}
