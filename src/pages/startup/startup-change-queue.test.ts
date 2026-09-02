import { describe, expect, it } from 'vitest';

import { enqueueStartupChange, queuedStartupItemIds, type StartupQueuedChange } from './startup-change-queue';

describe('startup change queue', () => {
  it('coalesces compatible changes with the same desired state', () => {
    const first = enqueueStartupChange([], ['a'], 'disabled', 3);
    const second = enqueueStartupChange(first, ['b', 'c'], 'disabled', 3);

    expect(second).toEqual([{ itemIds: ['a', 'b', 'c'], desiredState: 'disabled' }]);
  });

  it('keeps opposite desired states in separate batches', () => {
    const first = enqueueStartupChange([], ['a'], 'disabled', 3);
    const second = enqueueStartupChange(first, ['b'], 'enabled', 3);

    expect(second).toEqual([
      { itemIds: ['a'], desiredState: 'disabled' },
      { itemIds: ['b'], desiredState: 'enabled' },
    ]);
  });

  it('retains only the latest queued intent for an item', () => {
    const queue: StartupQueuedChange[] = [
      { itemIds: ['a', 'b'], desiredState: 'disabled' },
      { itemIds: ['c'], desiredState: 'enabled' },
    ];

    expect(enqueueStartupChange(queue, ['b'], 'enabled', 3)).toEqual([
      { itemIds: ['a'], desiredState: 'disabled' },
      { itemIds: ['c', 'b'], desiredState: 'enabled' },
    ]);
  });

  it('splits work at the backend batch limit and reports every pending item', () => {
    const queue = enqueueStartupChange([], ['a', 'b', 'c', 'd'], 'disabled', 3);

    expect(queue).toEqual([
      { itemIds: ['a', 'b', 'c'], desiredState: 'disabled' },
      { itemIds: ['d'], desiredState: 'disabled' },
    ]);
    expect(queuedStartupItemIds(queue[0]!, queue.slice(1))).toEqual(new Set(['a', 'b', 'c', 'd']));
  });

  it('deduplicates item identifiers without mutating the existing queue', () => {
    const existing: StartupQueuedChange[] = [{ itemIds: ['a'], desiredState: 'disabled' }];

    expect(enqueueStartupChange(existing, ['b', 'b'], 'disabled', 3)).toEqual([
      { itemIds: ['a', 'b'], desiredState: 'disabled' },
    ]);
    expect(existing).toEqual([{ itemIds: ['a'], desiredState: 'disabled' }]);
  });

  it('uses single-item batches when the configured limit is invalid', () => {
    expect(enqueueStartupChange([], ['a', 'b'], 'disabled', 0)).toEqual([
      { itemIds: ['a'], desiredState: 'disabled' },
      { itemIds: ['b'], desiredState: 'disabled' },
    ]);
  });

  it('does not merge a grouped change that requires review into ordinary queued work', () => {
    const ordinary = enqueueStartupChange([], ['a'], 'disabled', 3);

    expect(enqueueStartupChange(ordinary, ['b', 'c'], 'disabled', 3, true)).toEqual([
      { itemIds: ['a'], desiredState: 'disabled' },
      { itemIds: ['b', 'c'], desiredState: 'disabled', requiresReview: true },
    ]);
  });
});
