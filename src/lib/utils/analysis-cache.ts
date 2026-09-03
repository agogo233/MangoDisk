import type { AnalysisResult } from '@/lib/models/analysis';
import * as PathUtils from '@/lib/utils/path';
export function key(path: string): string {
  return PathUtils.comparisonKey(path);
}
export function touch(order: readonly string[], key: string): string[] {
  return [...order.filter(item => item !== key), key];
}
/**
 * The bounded recent-results cache accelerates navigation without copying
 * the native index. Eviction only causes a later result reload.
 */
export function store(
  cache: Readonly<Record<string, AnalysisResult>>,
  order: readonly string[],
  result: AnalysisResult,
  limit: number
): {
  cache: Record<string, AnalysisResult>;
  order: string[];
} {
  const cacheKey = key(result.root);
  const nextCache = { ...cache, [cacheKey]: result };
  const nextOrder = touch(order, cacheKey);
  while (nextOrder.length > limit) {
    const removedKey = nextOrder.shift();
    if (removedKey) delete nextCache[removedKey];
  }
  return { cache: nextCache, order: nextOrder };
}
export function retainExisting(order: readonly string[], cache: Readonly<Record<string, AnalysisResult>>): string[] {
  return order.filter(key => Boolean(cache[key]));
}
export function syncAfterDelete(
  cache: Record<string, AnalysisResult>,
  removedPath: string,
  releasedBytes: number,
  removedFileCount: number
): Record<string, AnalysisResult> {
  const removedKey = key(removedPath);
  const nextCache: Record<string, AnalysisResult> = {};
  for (const [resultKey, result] of Object.entries(cache)) {
    if (PathUtils.isSameOrChildKey(resultKey, removedKey)) continue;
    if (!PathUtils.isSameOrChildKey(removedKey, resultKey)) {
      nextCache[resultKey] = result;
      continue;
    }
    const entries = result.entries.flatMap(entry => {
      const entryKey = key(entry.path);
      if (entryKey === removedKey) return [];
      if (!PathUtils.isSameOrChildKey(removedKey, entryKey)) return [entry];
      return [
        {
          ...entry,
          bytes: Math.max(0, entry.bytes - releasedBytes),
          fileCount: Math.max(0, entry.fileCount - removedFileCount),
        },
      ];
    });
    nextCache[resultKey] = {
      ...result,
      totalBytes: Math.max(0, result.totalBytes - releasedBytes),
      entries,
    };
  }
  return nextCache;
}
