import { describe, expect, it } from 'vitest';
import type { PresentedOperationRecord } from '@/lib/models/history/history';
import { displayedReleasedBytes, usesItemCounts } from './operation-record';

function fileRecord(): Extract<PresentedOperationRecord, { category: 'largeFileCleanup' }> {
  return {
    schemaVersion: 1,
    operationId: 'operation',
    category: 'largeFileCleanup',
    startedAtMs: 0,
    finishedAtMs: 1000,
    outcome: 'completed',
    dryRun: false,
    selectedItemCount: 1,
    affectedItemCount: 1,
    expectedBytes: 4096,
    releasedBytes: 512,
    releasedBytesIsEstimate: false,
    failedItemCount: 0,
    details: { type: 'largeFileCleanup', payload: { items: [], omittedItemCount: 0 } },
  };
}

describe('history byte accounting', () => {
  it('preserves measured, estimated, and unavailable file-cleanup totals', () => {
    const record = fileRecord();
    expect(displayedReleasedBytes(record)).toBe(512);
    expect(displayedReleasedBytes({ ...record, releasedBytesIsEstimate: true })).toBe(4096);
    expect(displayedReleasedBytes({ ...record, releasedBytes: null })).toBe(0);
    expect(usesItemCounts(record)).toBe(false);
  });

  it('does not substitute the batch estimate for cancelled uninstall actions', () => {
    const record: Extract<PresentedOperationRecord, { category: 'applicationUninstall' }> = {
      ...fileRecord(),
      category: 'applicationUninstall',
      expectedBytes: 8192,
      releasedBytes: 8192,
      releasedBytesIsEstimate: true,
      details: {
        type: 'applicationUninstall',
        payload: {
          batchId: 'batch',
          restartRequired: false,
          applications: [
            {
              restartRequired: false,
              planId: 'plan',
              applicationId: 'application',
              applicationName: 'Example',
              applicationIdentifier: 'org.example.app',
              applicationVersion: null,
              applicationPublisher: null,
              applicationPlatform: 'macosBundle',
              installerKind: null,
              componentIds: ['binary', 'cache'],
              actions: [
                {
                  componentId: 'binary',
                  kind: 'applicationBinary',
                  status: 'cancelled',
                  reason: null,
                  expectedBytes: 7680,
                  releasedBytes: 0,
                },
                {
                  componentId: 'cache',
                  kind: 'cache',
                  status: 'completed',
                  reason: null,
                  expectedBytes: 512,
                  releasedBytes: 256,
                },
              ],
            },
          ],
        },
      },
    };
    expect(displayedReleasedBytes(record)).toBe(256);
    record.details.payload.applications[0]!.actions[1]!.releasedBytes = 0;
    expect(displayedReleasedBytes(record)).toBe(0);
  });
});
