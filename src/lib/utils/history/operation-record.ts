import type { PresentedOperationRecord } from '@/lib/models/history/history';
import { releasedActionBytes } from '@/lib/utils/applications/application-uninstall-result';

export function usesItemCounts(record: PresentedOperationRecord): boolean {
  return (
    record.category === 'startupManagement' ||
    record.category === 'systemOptimization' ||
    record.category === 'privacyCleanup'
  );
}

/** Uninstall action results retain verified bytes even when the batch total is an estimate. */
export function displayedReleasedBytes(record: PresentedOperationRecord): number {
  if (record.details.type === 'applicationUninstall') {
    return record.details.payload.applications.reduce(
      (total, application) => total + releasedActionBytes(application.actions),
      0
    );
  }
  return record.releasedBytesIsEstimate ? record.expectedBytes : (record.releasedBytes ?? 0);
}
