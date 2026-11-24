export function parseArgs(argv: string[]): {
  target: string | null
  expectedArch: string | null
  positionalPath: string | null
}

export function expectedArchFromTarget(target: string | null): 'x64' | 'arm64' | null

export function normalizeExpectedArch(arch: string | null): string | null

export function expectedDebArch(arch: string): 'amd64' | 'arm64'

export function inspectElfBuffer(
  bytes: Uint8Array,
  filePath?: string,
): {
  path: string
  bytes: number
  machine: number
  machineHex: string
  architecture: string
  sha256: string
}

export function collectPackagedElfBinaries(baseDir: string): string[]

export function parseDebControlMetadata(controlOutput: string): Record<string, string>

export function parseDebDependsList(dependsValue?: string): string[]

export function verifyDebControlMetadata(
  fields: Record<string, string>,
  options: { expectedVersion: string; expectedDebArchitecture: string },
): {
  package: string
  version: string
  architecture: string
  section: string | null
  depends: string[]
}

export function verifyDebFileList(
  listingOutput: string,
  expectedResources: string[],
): {
  binary: string
  desktopFile: string
  iconCount: number
  verifiedDebResources: number
}

export function expectedResourceFiles(): string[]

export function verifyLinuxBundle(argv?: string[]): Record<string, unknown>
