/**
 * Provides deterministic path presentation and comparison without touching
 * the filesystem. Platform discovery remains in services and Core.
 */
export function display(path: string): string {
  if (path.slice(0, 8).toLocaleUpperCase('en-US') === '\\\\?\\UNC\\') return `\\\\${path.slice(8)}`;
  if (path.slice(0, 4) === '\\\\?\\') return path.slice(4);
  return path;
}
export function fileName(path: string): string {
  const displayPath = display(path);
  const normalized = displayPath.replace(/[\\/]+$/, '');
  if (!normalized) return displayPath.startsWith('/') ? '/' : displayPath;
  return normalized.split(/[\\/]/).filter(Boolean).at(-1) ?? normalized;
}
/**
 * Cache keys fold case only for Windows paths and retain one separator for
 * roots. Unix keys preserve case for case-sensitive volumes.
 */
export function comparisonKey(path: string): string {
  const displayPath = display(path);
  const isWindowsPath = /^[A-Za-z]:[\\/]?/u.test(displayPath) || displayPath.startsWith('\\\\');
  const normalized = displayPath.replaceAll('/', '\\');
  const windowsDriveRoot = /^[A-Za-z]:\\+$/u.test(normalized);
  const withoutTrailingSeparators =
    normalized === '\\' || windowsDriveRoot ? normalized.replace(/\\+$/u, '\\') : normalized.replace(/\\+$/u, '');
  return isWindowsPath ? withoutTrailingSeparators.toLocaleLowerCase('en-US') : withoutTrailingSeparators;
}
export function isSameOrChildKey(pathKey: string, rootKey: string): boolean {
  if (pathKey === rootKey) return true;
  if (rootKey.endsWith('\\')) return pathKey.startsWith(rootKey);
  return pathKey.startsWith(`${rootKey}\\`);
}
/**
 * Reduces scan roots to the smallest set that covers the same filesystem
 * scope. A selected parent makes its descendants redundant; adding a parent
 * later also replaces descendants that were selected earlier.
 */
export function collapseOverlappingRoots(paths: string[]): string[] {
  return paths.map(display).reduce<string[]>((roots, path) => {
    const pathKey = comparisonKey(path);
    const alreadyCovered = roots.some(root => isSameOrChildKey(pathKey, comparisonKey(root)));
    if (alreadyCovered) return roots;
    return [...roots.filter(root => !isSameOrChildKey(comparisonKey(root), pathKey)), path];
  }, []);
}
