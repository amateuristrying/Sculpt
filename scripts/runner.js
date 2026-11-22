import { spawnSync, execSync } from 'node:child_process';
import path from 'node:path';
import fs from 'node:fs';

function needsVirtualDrive() {
  if (process.platform !== 'win32') return false;
  try {
    const real = fs.realpathSync('.');
    let current = path.dirname(real);
    while (current && path.dirname(current) !== current) {
      try {
        fs.readdirSync(current);
      } catch (err) {
        if (err && (err.code === 'EACCES' || err.code === 'EPERM')) {
          return true;
        }
      }
      current = path.dirname(current);
    }
  } catch {
    return true;
  }
  return false;
}

function getOrMountDrive() {
  const drive = 'S:';
  try {
    const substOutput = execSync('subst', { encoding: 'utf8' });
    if (!substOutput.includes(`${drive}\\:`)) {
      execSync(`subst ${drive} "${process.cwd()}"`);
    }
  } catch (e) {
    console.error('Failed to configure virtual drive for junction resolution:', e);
  }
  return `${drive}\\`;
}

const args = process.argv.slice(2);
if (args.length === 0) {
  process.exit(0);
}

let cwd = process.cwd();
if (needsVirtualDrive()) {
  cwd = getOrMountDrive();
}

const [cmd, ...cmdArgs] = args;
const isWindows = process.platform === 'win32';
const binName = isWindows ? `${cmd}.cmd` : cmd;
const localBin = path.join(cwd, 'node_modules', '.bin', binName);
const executable = fs.existsSync(localBin) ? localBin : cmd;

const result = spawnSync(executable, cmdArgs, {
  cwd,
  stdio: 'inherit',
  shell: true,
  env: process.env,
});

process.exit(result.status ?? 0);
