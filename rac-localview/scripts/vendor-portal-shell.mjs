#!/usr/bin/env node
/**
 * vendor-portal-shell.mjs — build the Portal shell and vendor it into
 * the native engine as an embedded asset. No dependencies.
 *
 * Runs the shell-only viewer build (`npm run build:viewer -- --shell-only`),
 * then writes into ../rust/rac-engine/assets/portal/:
 *
 *   asdecided-portal-shell.html   the shell, with a provenance comment
 *                            inserted immediately after the doctype
 *   provenance.json          { viewer_commit, shell_sha256,
 *                              viewer_source_sha256, vendored_with }
 *
 * viewer_source_sha256 — the drift-guard hash over the viewer source
 * tree. The algorithm is normative:
 *
 *   File set (paths relative to rac-localview/, POSIX separators):
 *     - src/viewer/** recursively, EXCLUDING src/viewer/sample/
 *     - src/components/** recursively
 *     - src/styles/** recursively
 *     - vite.config.viewer.ts
 *     - scripts/build-viewer-artifact.mjs
 *
 *   Algorithm:
 *     1. Collect the file set; sort the relative paths byte-wise
 *        (JavaScript default string sort / Python sorted()).
 *     2. Read each file as UTF-8 text and LF-normalise it
 *        (replace "\r\n" with "\n").
 *     3. Feed sha256 with, per file in sorted order:
 *        the relative path, a NUL byte ("\0"), the normalised
 *        content, a NUL byte ("\0") — i.e. the concatenation of
 *        "path\0content\0" for every file.
 *     4. viewer_source_sha256 is the lowercase hex digest.
 */

import { execSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { mkdirSync, readdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, relative, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '..');
const repoRoot = resolve(root, '..');
const portalDir = resolve(repoRoot, 'rust/rac-engine/assets/portal');
const shellBuildPath = resolve(root, 'dist/viewer/lore-portal-shell.html');

/* ---- 1. build the shell ------------------------------------------------ */

execSync('npm run build:viewer -- --shell-only', {
  cwd: root,
  stdio: 'inherit',
});

/* ---- 2. provenance inputs ---------------------------------------------- */

const commit = execSync('git rev-parse --short HEAD', {
  cwd: root,
  encoding: 'utf8',
}).trim();

/** Recursively collect files under dir, as paths relative to root. */
function collect(dir, exclude = []) {
  const out = [];
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const abs = resolve(dir, entry.name);
    const rel = relative(root, abs).split('\\').join('/');
    if (exclude.some((ex) => rel === ex || rel.startsWith(`${ex}/`))) continue;
    if (entry.isDirectory()) out.push(...collect(abs, exclude));
    else if (entry.isFile()) out.push(rel);
  }
  return out;
}

const sourceFiles = [
  ...collect(resolve(root, 'src/viewer'), ['src/viewer/sample']),
  ...collect(resolve(root, 'src/components')),
  ...collect(resolve(root, 'src/styles')),
  'vite.config.viewer.ts',
  'scripts/build-viewer-artifact.mjs',
].sort();

const sourceHash = createHash('sha256');
for (const rel of sourceFiles) {
  const content = readFileSync(resolve(root, rel), 'utf8').replaceAll('\r\n', '\n');
  sourceHash.update(rel);
  sourceHash.update('\0');
  sourceHash.update(content);
  sourceHash.update('\0');
}
const viewerSourceSha256 = sourceHash.digest('hex');

/* ---- 3. write the shell with its provenance comment -------------------- */

const shell = readFileSync(shellBuildPath, 'utf8');
const doctypeRe = /^<!doctype html>/i;
if (!doctypeRe.test(shell)) {
  console.error('vendor-portal-shell: shell does not start with a doctype');
  process.exit(1);
}
const provenanceComment =
  `<!-- Portal shell vendored from rac-localview @ ${commit}; ` +
  'rebuild: cd rac-localview && npm run vendor:shell -->';
const vendoredShell = shell.replace(
  doctypeRe,
  (doctype) => `${doctype}\n${provenanceComment}`,
);

mkdirSync(portalDir, { recursive: true });
const shellPath = resolve(portalDir, 'asdecided-portal-shell.html');
writeFileSync(shellPath, vendoredShell);

const shellSha256 = createHash('sha256').update(vendoredShell).digest('hex');

/* ---- 4. write the manifest --------------------------------------------- */

const provenance = {
  viewer_commit: commit,
  shell_sha256: shellSha256,
  viewer_source_sha256: viewerSourceSha256,
  vendored_with: 'rac-localview/scripts/vendor-portal-shell.mjs',
};
const provenancePath = resolve(portalDir, 'provenance.json');
writeFileSync(provenancePath, JSON.stringify(provenance, null, 2) + '\n');

console.log(`wrote ${shellPath}`);
console.log(`wrote ${provenancePath}`);
console.log(`  viewer_commit:        ${commit}`);
console.log(`  shell_sha256:         ${shellSha256}`);
console.log(`  viewer_source_sha256: ${viewerSourceSha256} (${sourceFiles.length} files)`);
