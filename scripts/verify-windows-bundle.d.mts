export function parseArgs(argv: string[]): {
  target: string | null
  expectedArch: string | null
  positionalPath: string | null
}

export function expectedArchFromTarget(target: string | null): 'x64' | 'arm64' | null

export function normalizeExpectedArch(arch: string | null): string | null

export function resolveReleaseDir(
  options: { target: string | null; positionalPath: string | null },
  rootDir?: string,
): string

export function inspectPeBuffer(
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

export function inspectPeBinary(filePath: string): {
  path: string
  bytes: number
  machine: number
  machineHex: string
  architecture: string
  sha256: string
}

export function collectPackagedBinaries(baseDir: string): string[]

export function expectedResourceFiles(rootDir?: string): string[]

export function verifyWindowsBundleConfig(windowsConfig: Record<string, unknown>): {
  targets: string[]
  installMode: string
  compression: string
}

export function verifyWindowsBundle(argv?: string[], rootDir?: string): Record<string, unknown>
