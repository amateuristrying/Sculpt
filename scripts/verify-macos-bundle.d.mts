export function parseArgs(argv: string[]): {
  target: string | null
  expectedArch: string | null
  positionalPath: string | null
  requireDmg: boolean
}

export function expectedArchFromTarget(target: string | null): 'arm64' | null

export function normalizeExpectedArch(arch: string | null): 'arm64' | null

export function parseLipoArchs(output: string): string[]

export function verifyMacosBundleConfig(
  baseConfig: Record<string, unknown>,
  macosConfig: Record<string, unknown>,
): {
  targets: string[]
  minimumSystemVersion: string
}

export function isExpectedAppleSiliconDmgName(
  fileName: string,
  productName: string,
  version: string,
): boolean

export function verifyMacosBundleLayout(
  bundleDir: string,
  productName: string,
  version: string,
  options?: { requireDmg?: boolean },
): {
  appPath: string
  dmgPath: string | null
}

export function expectedResourceFiles(rootDir?: string): string[]

export function verifyMacosBundle(argv?: string[], rootDir?: string): Record<string, unknown>
