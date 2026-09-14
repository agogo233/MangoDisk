/**
 * Derive compact system-surface icons from the approved public vector artwork.
 * Run with `node scripts/generate-resident-icons.mjs`; Tauri supplies the rasterizer
 * and ICO encoder, so icon maintenance needs no extra image-tool dependency.
 */
import { mkdtempSync, readFileSync, writeFileSync, mkdirSync, copyFileSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { execFileSync } from 'node:child_process';

const root = fileURLToPath(new URL('../', import.meta.url));
const source = readFileSync(join(root, 'public/mangodisk.svg'), 'utf8');
const groups = [...source.matchAll(/<g\b[^>]*>[\s\S]*?<\/g>/gu)].map(match => match[0]);
if (groups.length !== 4 || !groups.some(group => group.includes('url(#cream)'))) {
  throw new Error('The approved logo structure changed; review the compact icon derivation.');
}

// Color icons need optical compensation next to round and square Windows icons:
// reduce vertical padding to about 1.5% and widen the narrow mango by 15% around
// its center. Keep this adjustment out of the macOS template and source artwork.
const compact = source
  .replace('viewBox="0 0 1254 1254"', 'viewBox="122 77 1052 1052"')
  .replace('<g transform=', '<g transform="translate(-97.2 0) scale(1.15 1)"><g transform=')
  .replace('</svg>', '</g></svg>');
// Cream regions become transparent cutouts. Keeping only the outer orange,
// stem and leaf paths preserves the platter/arm silhouette in template mode.
const silhouette = groups
  .filter(group => !group.includes('url(#cream)'))
  .map(group => group.replace(/fill="url\(#[a-z]+\)"/gu, 'fill="black"'))
  .join('\n');
const template = `<svg xmlns="http://www.w3.org/2000/svg" width="36" height="36" viewBox="92 46 1112 1112">${silhouette}</svg>`;
const temporary = mkdtempSync(join(tmpdir(), 'mangodisk-icons-'));
const output = join(root, 'src-tauri/icons');
const cli = join(root, 'node_modules/@tauri-apps/cli/tauri.js');
function generate(input, directory, sizes = []) {
  execFileSync(
    process.execPath,
    [cli, 'icon', input, '--output', directory, ...sizes.flatMap(size => ['--png', String(size)])],
    {
      cwd: root,
      stdio: 'inherit',
    }
  );
}
try {
  const colorSource = join(temporary, 'color.svg');
  const templateSource = join(temporary, 'template.svg');
  writeFileSync(colorSource, compact);
  writeFileSync(templateSource, template);
  generate(templateSource, join(temporary, 'template'), [36]);
  generate(colorSource, join(temporary, 'color'), [64]);
  generate(colorSource, join(temporary, 'application'));
  copyFileSync(join(temporary, 'template/36x36.png'), join(output, 'tray-template.png'));
  copyFileSync(join(temporary, 'color/64x64.png'), join(output, 'tray-color.png'));
  mkdirSync(join(output, 'windows'), { recursive: true });
  // Tauri embeds the first ICO entry as the live window icon. Put the largest
  // raster first to avoid enlarging 32 px pixels on high-DPI taskbars. Windows
  // still selects the appropriate size from the full executable icon directory.
  const ico = readFileSync(join(temporary, 'application/icon.ico'));
  const count = ico.readUInt16LE(4);
  const entries = Array.from({ length: count }, (_, index) => ico.subarray(6 + index * 16, 22 + index * 16));
  entries.sort((left, right) => (right[0] || 256) - (left[0] || 256));
  writeFileSync(
    join(output, 'windows/icon.ico'),
    Buffer.concat([ico.subarray(0, 6), ...entries, ico.subarray(6 + count * 16)])
  );
  console.log('Generated macOS template, color tray, and Windows application icons.');
} finally {
  rmSync(temporary, { recursive: true, force: true });
}
