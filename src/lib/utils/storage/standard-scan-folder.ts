import type { StandardScanFolder } from '@/lib/models/storage/standard-scan-folder';
import * as PathUtils from '@/lib/utils/filesystem/path';

/**
 * Finds a standard folder using the current platform's path rules.
 *
 * A standard folder's real path usually remains unchanged when the app locale changes. For example,
 * a localized Downloads label may still map to `Downloads` on disk. Callers should use the stable ID
 * to resolve localized text instead of displaying the final path segment directly.
 */
export function findStandardScanFolderByPath(
  folders: readonly StandardScanFolder[],
  path: string
): StandardScanFolder | null {
  const pathKey = PathUtils.comparisonKey(path);
  return folders.find(folder => PathUtils.comparisonKey(folder.path) === pathKey) ?? null;
}
