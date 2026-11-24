import { mkdtempSync, mkdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join, resolve } from 'node:path'
import { describe, expect, it } from 'vitest'
import {
  expectedArchFromTarget as expectedWinArch,
  inspectPeBuffer,
  parseArgs as parseWinArgs,
  verifyWindowsBundleConfig,
} from '../../scripts/verify-windows-bundle.mjs'
import {
  expectedArchFromTarget as expectedMacArch,
  isExpectedAppleSiliconDmgName,
  parseArgs as parseMacArgs,
  parseLipoArchs,
  verifyMacosBundleConfig,
  verifyMacosBundleLayout,
} from '../../scripts/verify-macos-bundle.mjs'

const REPO_ROOT = resolve(import.meta.dirname, '../..')

function makePeHeader(machine: number) {
  const buf = Buffer.alloc(128)
  buf.write('MZ', 0, 'ascii')
  buf.writeUInt32LE(0x40, 0x3c)
  buf.write('PE\0\0', 0x40, 'ascii')
  buf.writeUInt16LE(machine, 0x44)
  return buf
}

describe('Minimal package matrix verifiers (Windows NSIS & macOS Apple Silicon DMG)', () => {
  it('validates live Windows and macOS Tauri configs and rejects deferred targets', () => {
    const baseConf = JSON.parse(readFileSync(join(REPO_ROOT, 'src-tauri/tauri.conf.json'), 'utf8'))
    const winConf = JSON.parse(readFileSync(join(REPO_ROOT, 'src-tauri/tauri.windows.conf.json'), 'utf8'))
    const macConf = JSON.parse(readFileSync(join(REPO_ROOT, 'src-tauri/tauri.macos.conf.json'), 'utf8'))

    expect(verifyWindowsBundleConfig(winConf)).toEqual({
      targets: ['nsis'],
      installMode: 'currentUser',
      compression: 'lzma',
    })
    expect(verifyMacosBundleConfig(baseConf, macConf)).toEqual({
      targets: ['app', 'dmg'],
      minimumSystemVersion: '12.0',
    })

    expect(() =>
      verifyWindowsBundleConfig({
        bundle: { targets: ['nsis', 'msi'], windows: { nsis: { installMode: 'currentUser', compression: 'lzma' } } },
      }),
    ).toThrow(/nsis/)

    expect(() =>
      verifyWindowsBundleConfig({
        bundle: { targets: ['nsis'], windows: { nsis: { installMode: 'both', compression: 'lzma' } } },
      }),
    ).toThrow(/currentUser/)

    expect(() =>
      verifyMacosBundleConfig(baseConf, {
        bundle: { targets: ['app', 'pkg'], macOS: { minimumSystemVersion: '12.0' } },
      }),
    ).toThrow(/app.*dmg/)
  })

  it('parses Windows PE32+ headers for x64, arm64, and x86 installer stubs', () => {
    expect(parseWinArgs(['--target', 'aarch64-pc-windows-msvc', '--expected-arch=arm64'])).toEqual({
      target: 'aarch64-pc-windows-msvc',
      expectedArch: 'arm64',
      positionalPath: null,
    })
    expect(expectedWinArch('x86_64-pc-windows-msvc')).toBe('x64')
    expect(expectedWinArch('aarch64-pc-windows-msvc')).toBe('arm64')

    expect(inspectPeBuffer(makePeHeader(0x8664), 'sculpt-x64.exe')).toMatchObject({
      architecture: 'x64',
      machineHex: '0x8664',
    })
    expect(inspectPeBuffer(makePeHeader(0xaa64), 'sculpt-arm64.exe')).toMatchObject({
      architecture: 'arm64',
      machineHex: '0xaa64',
    })
    expect(inspectPeBuffer(makePeHeader(0x14c), 'setup.exe')).toMatchObject({
      architecture: 'x86',
      machineHex: '0x14c',
    })
    expect(() => inspectPeBuffer(Buffer.from('ELF'), 'bad.exe')).toThrow(/truncated/)
  })

  it('enforces Apple Silicon arm64-only macOS .app and .dmg bundles', () => {
    expect(parseMacArgs(['--target', 'aarch64-apple-darwin', '--expected-arch=arm64'])).toEqual({
      target: 'aarch64-apple-darwin',
      expectedArch: 'arm64',
      positionalPath: null,
      requireDmg: true,
    })
    expect(expectedMacArch('aarch64-apple-darwin')).toBe('arm64')
    expect(() => expectedMacArch('x86_64-apple-darwin')).toThrow(/Apple Silicon/)
    expect(() => expectedMacArch('universal-apple-darwin')).toThrow(/Apple Silicon/)

    expect(parseLipoArchs('arm64\n')).toEqual(['arm64'])
    expect(() => parseLipoArchs('x86_64\n')).toThrow(/arm64 only/)
    expect(() => parseLipoArchs('x86_64 arm64\n')).toThrow(/arm64 only/)

    expect(isExpectedAppleSiliconDmgName('Sculpt_0.1.0_aarch64.dmg', 'Sculpt', '0.1.0')).toBe(true)
    expect(isExpectedAppleSiliconDmgName('Sculpt_0.1.0_x86_64.dmg', 'Sculpt', '0.1.0')).toBe(false)
    expect(isExpectedAppleSiliconDmgName('Sculpt_0.1.0_universal.dmg', 'Sculpt', '0.1.0')).toBe(false)

    const tempDir = mkdtempSync(join(tmpdir(), 'sculpt-mac-bundle-'))
    try {
      const bundleDir = join(tempDir, 'bundle')
      const appDir = join(bundleDir, 'macos', 'Sculpt.app')
      const dmgDir = join(bundleDir, 'dmg')
      mkdirSync(appDir, { recursive: true })
      mkdirSync(dmgDir, { recursive: true })
      writeFileSync(join(dmgDir, 'Sculpt_0.1.0_aarch64.dmg'), Buffer.from('DMG payload'))

      const layout = verifyMacosBundleLayout(bundleDir, 'Sculpt', '0.1.0')
      expect(layout.appPath).toBe(appDir)
      expect(layout.dmgPath).toBe(join(dmgDir, 'Sculpt_0.1.0_aarch64.dmg'))

      writeFileSync(join(bundleDir, 'Sculpt-macOS-arm64.zip'), Buffer.from('zip'))
      expect(() => verifyMacosBundleLayout(bundleDir, 'Sculpt', '0.1.0')).toThrow(/only \.app and \.dmg/)
      rmSync(join(bundleDir, 'Sculpt-macOS-arm64.zip'))

      mkdirSync(join(bundleDir, 'pkg'), { recursive: true })
      expect(() => verifyMacosBundleLayout(bundleDir, 'Sculpt', '0.1.0')).toThrow(/\.pkg/)
    } finally {
      rmSync(tempDir, { recursive: true, force: true })
    }
  })
})
