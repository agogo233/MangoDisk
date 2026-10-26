import type { VolumeInfo } from '@/lib/models/storage/volume';
import * as PathUtils from '@/lib/utils/filesystem/path';
export function findForPath(disks: VolumeInfo[], path: string, fallback: VolumeInfo | null = null): VolumeInfo | null {
  const normalizedPath = PathUtils.comparisonKey(path);
  if (!normalizedPath) return fallback;
  // Prefer specific mounts and enforce path boundaries so similarly prefixed
  // volumes cannot match one another.
  return (
    [...disks]
      .sort((left, right) => PathUtils.display(right.mountPoint).length - PathUtils.display(left.mountPoint).length)
      .find(disk => {
        const mountPoint = PathUtils.comparisonKey(disk.mountPoint);
        return PathUtils.isSameOrChildKey(normalizedPath, mountPoint);
      }) ?? fallback
  );
}
