export interface CiHygieneArgs {
  checkRepo: boolean
  checkArtifact: string | null
  artifactGlob: string | null
  writeChecksums: string | null
}

export function parseArgs(argv: string[]): CiHygieneArgs
export function validateGitignore(gitignoreText: string): string[]
export function scanPublicFilesForLeaks(rootDir?: string): Array<{ file: string; line: number }>
export function parseWorkflowJobs(workflowText: string): {
  topLevel: string
  jobs: Record<string, string>
}
export function validateCiWorkflow(workflowText: string): {
  jobs: Record<string, { runsOn: string; timeoutMinutes: number; artifactName: string | null }>
  uploadStepCount: number
}
export function validateTauriBundleConfigs(rootDir?: string): {
  resourceCount: number
  windowsTargets: string[]
  linuxTargets: string[]
}
export function isForbiddenArtifactEntry(relativePath: string): string | null
export function verifyArtifactDirectory(
  releaseDir: string,
  options?: { artifactGlob?: string | null; writeChecksums?: string | null },
): {
  releaseDir: string
  forbiddenHits: Array<{ path: string; reason: string }>
  matchedArtifacts: Array<{ relativePath: string; fileName: string; bytes: number; sha256: string }>
  checksumManifest: string | null
}
export function runCiHygiene(argv?: string[], rootDir?: string): Record<string, unknown>
