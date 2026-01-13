import { execSync } from 'node:child_process';
import { chmodSync, existsSync } from 'node:fs';
import path from 'node:path';

function getRepoRoot() {
  return execSync('git rev-parse --show-toplevel', { encoding: 'utf8' }).trim();
}

function main() {
  const repoRoot = getRepoRoot();
  execSync('git config core.hooksPath .githooks', {
    cwd: repoRoot,
    stdio: 'ignore',
  });

  const hookPath = path.join(repoRoot, '.githooks', 'pre-commit');
  if (!existsSync(hookPath)) return;

  try {
    chmodSync(hookPath, 0o755);
  } catch {
    // ignore
  }
}

main();

