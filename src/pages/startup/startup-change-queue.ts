import type { StartupChangeSelection, StartupDesiredState } from '@/lib/models/startup';

export type StartupQueuedChange = Omit<StartupChangeSelection, 'scanId'> & {
  requiresReview?: boolean;
};

export function enqueueStartupChange(
  queue: readonly StartupQueuedChange[],
  itemIds: readonly string[],
  desiredState: StartupDesiredState,
  maximumBatchSize: number,
  requiresReview = false
): StartupQueuedChange[] {
  const uniqueItemIds = [...new Set(itemIds)];
  if (!uniqueItemIds.length) return [...queue];

  const replacedIds = new Set(uniqueItemIds);
  const next = queue
    .map(change => ({ ...change, itemIds: change.itemIds.filter(itemId => !replacedIds.has(itemId)) }))
    .filter(change => change.itemIds.length > 0);
  const batchSize = Number.isSafeInteger(maximumBatchSize) && maximumBatchSize > 0 ? maximumBatchSize : 1;

  for (let offset = 0; offset < uniqueItemIds.length; offset += batchSize) {
    const incoming = uniqueItemIds.slice(offset, offset + batchSize);
    let mergeTarget: StartupQueuedChange | undefined;
    for (let index = next.length - 1; index >= 0; index -= 1) {
      const candidate = next[index]!;
      if (
        candidate.desiredState === desiredState &&
        Boolean(candidate.requiresReview) === requiresReview &&
        candidate.itemIds.length + incoming.length <= batchSize
      ) {
        mergeTarget = candidate;
        break;
      }
    }
    if (mergeTarget) {
      mergeTarget.itemIds = [...mergeTarget.itemIds, ...incoming];
    } else {
      next.push({ itemIds: incoming, desiredState, ...(requiresReview ? { requiresReview: true } : {}) });
    }
  }

  return next;
}

export function queuedStartupItemIds(
  activeChange: StartupQueuedChange | null,
  queue: readonly StartupQueuedChange[]
): ReadonlySet<string> {
  return new Set([...(activeChange?.itemIds ?? []), ...queue.flatMap(change => change.itemIds)]);
}
