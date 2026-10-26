import type {
  ApplicationUninstallBatchResult,
  ApplicationUninstallActionResult,
  ApplicationUninstallScanResult,
} from '@/lib/models/applications/application';
export function applyVerifiedRemovals(
  catalog: ApplicationUninstallScanResult,
  result: ApplicationUninstallBatchResult
): ApplicationUninstallScanResult {
  if (result.dryRun) return catalog;
  const removedApplicationIds = new Set(
    result.results
      .filter(application =>
        application.actions.some(
          action =>
            (action.status === 'completed' &&
              (action.kind === 'applicationBinary' || action.kind === 'nativeInstaller')) ||
            // The native postflight can verify removal even when the vendor returns an error.
            // Remove only that stale row; the result and history retain their failed status.
            (action.status === 'failed' &&
              action.kind === 'nativeInstaller' &&
              action.reason === 'nativeInstallerFailedAfterRemoval')
        )
      )
      .map(application => application.applicationId)
  );
  if (!removedApplicationIds.size) return catalog;
  return removeApplications(catalog, removedApplicationIds);
}

/** Reconcile verified removals while preserving the scan snapshot and unaffected rows. */
export function removeApplications(
  catalog: ApplicationUninstallScanResult,
  removedApplicationIds: ReadonlySet<string>
): ApplicationUninstallScanResult {
  const candidates = catalog.candidates.filter(candidate => !removedApplicationIds.has(candidate.applicationId));
  if (candidates.length === catalog.candidates.length) return catalog;
  const readyCount = candidates.filter(
    candidate =>
      candidate.capability === 'ready' ||
      (candidate.platform === 'windowsRegistry' && candidate.capability === 'requiresElevation')
  ).length;
  return {
    ...catalog,
    candidates,
    readyCount,
    blockedCount: candidates.length - readyCount,
  };
}

/** Counts continuing installers separately from confirmed cancellation in mixed batches. */
export function summarizeBatchOutcome(result: ApplicationUninstallBatchResult) {
  const continuingApplications = result.results.filter(application =>
    application.actions.some(action => action.reason === 'externalUninstallerContinuing')
  ).length;
  const cancelledApplications = result.results.filter(
    application =>
      application.actions.some(action => action.status === 'cancelled') &&
      !application.actions.some(action => action.reason === 'externalUninstallerContinuing')
  ).length;
  const failed = result.failedApplicationCount;
  const completed = result.affectedApplicationCount;
  const hasFailures = result.failedItemCount > 0 || failed > 0;
  const onlyCancelled = cancelledApplications > 0 && !completed && !hasFailures && !continuingApplications;
  return { continuingApplications, cancelledApplications, failed, completed, hasFailures, onlyCancelled };
}

export function expectedActionBytes(actions: readonly ApplicationUninstallActionResult[]): number {
  return actions.reduce((total, action) => total + action.expectedBytes, 0);
}

export function releasedActionBytes(actions: readonly ApplicationUninstallActionResult[]): number {
  return actions.reduce((total, action) => total + action.releasedBytes, 0);
}
