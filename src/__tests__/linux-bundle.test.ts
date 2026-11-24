import { describe, expect, it } from 'vitest'
import {
  expectedArchFromTarget,
  expectedDebArch,
  expectedResourceFiles,
  inspectElfBuffer,
  parseArgs,
  parseDebControlMetadata,
  verifyDebControlMetadata,
  verifyDebFileList,
} from '../../scripts/verify-linux-bundle.mjs'

function makeElf64Header(machine: number) {
  const buf = Buffer.alloc(64)
  buf[0] = 0x7f
  buf.write('ELF', 1, 'ascii')
  buf[4] = 2 // 64-bit
  buf[5] = 1 // little-endian
  buf[6] = 1 // ELF version
  buf.writeUInt16LE(machine, 18)
  return buf
}

describe('Linux .deb bundle verifier helpers', () => {
  it('parses CLI arguments and maps target triples cleanly', () => {
    expect(parseArgs(['--target', 'x86_64-unknown-linux-gnu', '--expected-arch=x64'])).toEqual({
      target: 'x86_64-unknown-linux-gnu',
      expectedArch: 'x64',
      positionalPath: null,
    })
    expect(expectedArchFromTarget('x86_64-unknown-linux-gnu')).toBe('x64')
    expect(expectedArchFromTarget('aarch64-unknown-linux-gnu')).toBe('arm64')
    expect(expectedDebArch('x64')).toBe('amd64')
    expect(expectedDebArch('arm64')).toBe('arm64')
  })

  it('inspects ELF64 x86_64 and aarch64 headers and rejects truncated or invalid binaries', () => {
    const x64Info = inspectElfBuffer(makeElf64Header(0x3e), 'sculpt')
    expect(x64Info.architecture).toBe('x64')
    expect(x64Info.machineHex).toBe('0x3e')
    expect(x64Info.sha256).toHaveLength(64)

    const armInfo = inspectElfBuffer(makeElf64Header(0xb7), 'sculpt-arm')
    expect(armInfo.architecture).toBe('arm64')
    expect(armInfo.machineHex).toBe('0xb7')

    expect(() => inspectElfBuffer(Buffer.from('MZ\x00\x00'.padEnd(64, '\0')), 'win.exe')).toThrow(
      /ELF magic/,
    )
    expect(() => inspectElfBuffer(Buffer.alloc(8), 'tiny')).toThrow(/truncated/)
  })

  it('validates dpkg-deb control metadata with WebKitGTK 4.1 and rejects legacy 4.0', () => {
    const control = [
      'Package: sculpt',
      'Version: 0.1.0',
      'Architecture: amd64',
      'Section: graphics',
      'Priority: optional',
      'Depends: libwebkit2gtk-4.1-0 (>= 2.38), libgtk-3-0 (>= 3.24), libc6 (>= 2.35)',
    ].join('\n')

    const meta = parseDebControlMetadata(control)
    const verified = verifyDebControlMetadata(meta, {
      expectedVersion: '0.1.0',
      expectedDebArchitecture: 'amd64',
    })
    expect(verified.package).toBe('sculpt')
    expect(verified.architecture).toBe('amd64')
    expect(verified.depends).toContain('libwebkit2gtk-4.1-0')
    expect(verified.depends).toContain('libgtk-3-0')

    const legacyControl = control.replace('libwebkit2gtk-4.1-0 (>= 2.38)', 'libwebkit2gtk-4.0-37')
    expect(() =>
      verifyDebControlMetadata(parseDebControlMetadata(legacyControl), {
        expectedVersion: '0.1.0',
        expectedDebArchitecture: 'amd64',
      }),
    ).toThrow(/libwebkit2gtk-4\.1-0/)
  })

  it('validates .deb payload archive entries including binary, desktop file, icons, and backend resources', () => {
    const resources = expectedResourceFiles()
    expect(resources).toContain('LICENSE')
    expect(resources).toContain('backend/worker.py')
    expect(resources).toContain('backend/runtime-spec.json')

    const listing = [
      '-rwxr-xr-x root/root   9500000 2026-11-24 18:00 ./usr/bin/sculpt',
      '-rw-r--r-- root/root       320 2026-11-24 18:00 ./usr/share/applications/Sculpt.desktop',
      '-rw-r--r-- root/root      3711 2026-11-24 18:00 ./usr/share/icons/hicolor/128x128/apps/sculpt.png',
      ...resources.map((res: string) => `-rw-r--r-- root/root       100 2026-11-24 18:00 ./usr/lib/Sculpt/${res}`),
    ].join('\n')

    const result = verifyDebFileList(listing, resources)
    expect(result.binary).toBe('/usr/bin/sculpt')
    expect(result.desktopFile).toBe('/usr/share/applications/Sculpt.desktop')
    expect(result.iconCount).toBe(1)
    expect(result.verifiedDebResources).toBe(resources.length)

    const missingDesktop = listing.replace('./usr/share/applications/Sculpt.desktop\n', '')
    expect(() => verifyDebFileList(missingDesktop, resources)).toThrow(/\.desktop/)
  })
})
