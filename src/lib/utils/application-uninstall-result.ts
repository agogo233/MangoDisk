import type { ApplicationUninstallBatchResult, ApplicationUninstallScanResult } from '@/lib/models/application';
export function apply(
  catalog: ApplicationUninstallScanResult,
  result: ApplicationUninstallBatchResult
): ApplicationUninstallScanResult {
  if (result.dryRun) return catalog;
  const removedApplicationIds = new Set(
    result.results
      .filter(application =>
        application.actions.some(
          action =>
            action.status === 'completed' && (action.kind === 'applicationBinary' || action.kind === 'nativeInstaller')
        )
      )
      .map(application => application.applicationId)
  );
  if (!removedApplicationIds.size) return catalog;
  const candidates = catalog.candidates.filter(candidate => !removedApplicationIds.has(candidate.applicationId));
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
