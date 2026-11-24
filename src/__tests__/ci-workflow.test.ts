import { createHash } from 'node:crypto'
import { mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join, resolve } from 'node:path'
import { describe, expect, it } from 'vitest'
import {
  isForbiddenArtifactEntry,
  parseArgs,
  scanPublicFilesForLeaks,
  validateCiWorkflow,
  validateGitignore,
  validateTauriBundleConfigs,
  verifyArtifactDirectory,
} from '../../scripts/verify-ci-hygiene.mjs'

const REPO_ROOT = resolve(import.meta.dirname, '../..')
const PRIVATE_PLANNING_DIR = ['.', 'agents'].join('')

describe('CI workflow, privacy hygiene, and candidate artifact checks', () => {
  it('parses CLI options cleanly', () => {
    expect(parseArgs([])).toEqual({
      checkRepo: true,
      checkArtifact: null,
      artifactGlob: null,
      writeChecksums: null,
    })
    expect(
      parseArgs([
        '--check-artifact',
        'src-tauri/target/x86_64-pc-windows-msvc/release',
        '--artifact-glob=bundle/nsis/*-setup.exe',
        '--write-checksums',
        'SHA256SUMS.txt',
      ]),
    ).toEqual({
      checkRepo: false,
      checkArtifact: 'src-tauri/target/x86_64-pc-windows-msvc/release',
      artifactGlob: 'bundle/nsis/*-setup.exe',
      writeChecksums: 'SHA256SUMS.txt',
    })
  })

  it('validates the live .github/workflows/ci.yml, .gitignore, Tauri bundles, and zero public leaks', () => {
    const workflow = readFileSync(join(REPO_ROOT, '.github/workflows/ci.yml'), 'utf8')
    const gitignore = readFileSync(join(REPO_ROOT, '.gitignore'), 'utf8')

    const ignores = validateGitignore(gitignore)
    expect(ignores).toContain(`${PRIVATE_PLANNING_DIR}/`)
    expect(ignores).toContain('.sculpt-runtime/')

    expect(scanPublicFilesForLeaks(REPO_ROOT)).toEqual([])

    const summary = validateCiWorkflow(workflow)
    expect(summary.jobs['static-hygiene']?.runsOn).toBe('ubuntu-22.04')
    expect(summary.jobs['windows-x64']).toEqual({
      runsOn: 'windows-latest',
      timeoutMinutes: 40,
      artifactName: 'Sculpt-Windows-x64-shell',
    })
    expect(summary.jobs['windows-arm64']).toEqual({
      runsOn: 'windows-11-arm',
      timeoutMinutes: 40,
      artifactName: 'Sculpt-Windows-arm64-shell',
    })
    expect(summary.jobs['ubuntu-x64']).toEqual({
      runsOn: 'ubuntu-22.04',
      timeoutMinutes: 40,
      artifactName: 'Sculpt-Ubuntu-22.04-x64-shell',
    })
    expect(summary.jobs['macos-arm64']).toEqual({
      runsOn: 'macos-15',
      timeoutMinutes: 40,
      artifactName: 'Sculpt-macOS-arm64-preview',
    })

    const bundleSummary = validateTauriBundleConfigs(REPO_ROOT)
    expect(bundleSummary.windowsTargets).toEqual(['nsis'])
    expect(bundleSummary.windowsInstallMode).toBe('currentUser')
    expect(bundleSummary.linuxTargets).toEqual(['deb'])
    expect(bundleSummary.macosTargets).toEqual(['app', 'dmg'])
    expect(bundleSummary.resourceCount).toBeGreaterThanOrEqual(8)
  })

  it('rejects release publication, deploy steps, write permissions, deferred package targets, and private references in workflows', () => {
    const baseline = readFileSync(join(REPO_ROOT, '.github/workflows/ci.yml'), 'utf8')

    expect(() =>
      validateCiWorkflow(`${baseline}\n      - run: gh release create v0.1.0\n`),
    ).toThrow(/gh release/)

    expect(() =>
      validateCiWorkflow(`${baseline}\n      - uses: softprops/action-gh-release@v2\n`),
    ).toThrow(/softprops\/action-gh-release/)

    expect(() =>
      validateCiWorkflow(`${baseline}\n      - uses: tauri-apps/tauri-action@v0\n`),
    ).toThrow(/tauri-apps\/tauri-action/)

    expect(() =>
      validateCiWorkflow(baseline.replace('contents: read', 'contents: write')),
    ).toThrow(/write permissions/)

    expect(() =>
      validateCiWorkflow(baseline.replace("HF_HUB_OFFLINE: '1'", '# offline disabled')),
    ).toThrow(/HF_HUB_OFFLINE/)

    expect(() =>
      validateCiWorkflow(`${baseline}\n# leaked ${PRIVATE_PLANNING_DIR}/roadmap.md\n`),
    ).toThrow(/private planning/)

    expect(() =>
      validateCiWorkflow(baseline.replace('retention-days: 14', 'retention-days: 90')),
    ).toThrow(/retention-days/)

    expect(() =>
      validateCiWorkflow(baseline.replace('--bundles nsis', '--bundles msi')),
    ).toThrow(/package matrix flag/)

    expect(() =>
      validateCiWorkflow(baseline.replace('--bundles deb', '--bundles appimage')),
    ).toThrow(/package matrix flag/)

    expect(() =>
      validateCiWorkflow(baseline.replace('--target aarch64-apple-darwin', '--target x86_64-apple-darwin')),
    ).toThrow(/deferred\/forbidden package target/)

    expect(() =>
      validateCiWorkflow(baseline.replace('--bundles app,dmg', '--bundles pkg')),
    ).toThrow(/package matrix flag/)
  })

  it('rejects incomplete .gitignore rules', () => {
    expect(() => validateGitignore('node_modules/\n')).toThrow(/missing required exclusion/)
    expect(() => validateGitignore(`${PRIVATE_PLANNING_DIR}/\n.env\n`)).toThrow(/\.sculpt-runtime\//)
  })

  it('audits candidate artifact directories for forbidden caches/user data and writes SHA256SUMS.txt', () => {
    const tempDir = mkdtempSync(join(tmpdir(), 'sculpt-ci-hygiene-'))
    try {
      const releaseDir = join(tempDir, 'release')
      const nsisDir = join(releaseDir, 'bundle', 'nsis')
      const backendDir = join(releaseDir, 'backend', 'sculpt_backend')
      const cargoDeps = join(releaseDir, 'deps')
      mkdirSync(nsisDir, { recursive: true })
      mkdirSync(backendDir, { recursive: true })
      mkdirSync(cargoDeps, { recursive: true })

      const installerBytes = Buffer.from('NSIS candidate payload')
      const installerPath = join(nsisDir, 'Sculpt_0.1.0_x64-setup.exe')
      writeFileSync(installerPath, installerBytes)
      writeFileSync(join(backendDir, 'protocol.py'), 'PROTOCOL = 1\n')
      writeFileSync(join(cargoDeps, 'ignored.pyc'), 'cargo build internals ignored')

      const checksumPath = join(nsisDir, 'SHA256SUMS.txt')
      const result = verifyArtifactDirectory(releaseDir, {
        artifactGlob: 'bundle/nsis/*-setup.exe',
        writeChecksums: checksumPath,
      })

      expect(result.forbiddenHits).toEqual([])
      expect(result.matchedArtifacts).toHaveLength(1)
      const expectedSha = createHash('sha256').update(installerBytes).digest('hex')
      expect(result.matchedArtifacts[0].sha256).toBe(expectedSha)
      expect(readFileSync(checksumPath, 'utf8')).toBe(`${expectedSha}  Sculpt_0.1.0_x64-setup.exe\n`)

      // Forbidden entries inside the candidate output must fail loudly.
      expect(isForbiddenArtifactEntry('.env.example')).toBeNull()
      expect(isForbiddenArtifactEntry('.env.local')).toMatch(/forbidden env file/)
      expect(isForbiddenArtifactEntry('backend/__pycache__/worker.cpython-311.pyc')).toMatch(/__pycache__/)
      expect(isForbiddenArtifactEntry('models/triposr/model.safetensors')).toMatch(/\.safetensors/)
      expect(isForbiddenArtifactEntry(`${PRIVATE_PLANNING_DIR}/status.md`)).toMatch(/forbidden segment/)

      writeFileSync(join(releaseDir, '.sculpt-runtime-marker'), 'ok')
      writeFileSync(join(backendDir, 'secret.env'), 'TOKEN=nope')
      expect(() => verifyArtifactDirectory(releaseDir)).toThrow(/forbidden files/)
      rmSync(join(backendDir, 'secret.env'))

      mkdirSync(join(releaseDir, '.sculpt-runtime'), { recursive: true })
      expect(() => verifyArtifactDirectory(releaseDir)).toThrow(/\.sculpt-runtime/)
    } finally {
      rmSync(tempDir, { recursive: true, force: true })
    }
  })
})
