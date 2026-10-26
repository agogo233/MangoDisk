import type { Composer } from 'vue-i18n';
import type { ApplicationUninstallBatchResult } from '@/lib/models/applications/application';
import { summarizeBatchOutcome } from '@/lib/utils/applications/application-uninstall-result';

export const UNINSTALL_RESULT_TOAST_ID = 'application-uninstall-result';

/** The page owns localization and delivery; classification uses the shared result semantics. */
export function applicationUninstallNotification(
  result: ApplicationUninstallBatchResult,
  t: Composer['t'],
  format: { integer: (value: number) => string; bytes: (value: number) => string }
) {
  const { continuingApplications, cancelledApplications, failed, completed, hasFailures, onlyCancelled } =
    summarizeBatchOutcome(result);
  const title = hasFailures
    ? t('applicationUninstall.completedWithWarnings')
    : continuingApplications
      ? t('applicationUninstall.executionStoppedTitle')
      : onlyCancelled
        ? t('applicationUninstall.cancelledTitle')
        : cancelledApplications
          ? t('applicationUninstall.executionFinishedTitle')
          : t('applicationUninstall.completed');

  // Cancellation is one result, not an early exit: mixed batches must retain failures
  // and restart requirements. A confirmed cancellation needs no redundant explanation.
  const details: string[] = [];
  if (!onlyCancelled || cancelledApplications > 1) {
    if (completed) details.push(t('applicationUninstall.resultCompleted', { count: format.integer(completed) }));
    if (failed) details.push(t('applicationUninstall.resultFailed', { count: format.integer(failed) }));
    if (cancelledApplications)
      details.push(t('applicationUninstall.resultCancelled', { count: format.integer(cancelledApplications) }));
    if (continuingApplications)
      details.push(t('applicationUninstall.resultContinuing', { count: format.integer(continuingApplications) }));
    if (result.releasedBytes > 0) {
      details.push(
        t(
          result.releasedBytesIsEstimate
            ? 'applicationUninstall.resultEstimatedSpace'
            : 'applicationUninstall.resultReleasedSpace',
          { size: format.bytes(result.releasedBytes) }
        )
      );
    }
  }
  if (
    result.results.length === 1 &&
    result.results[0]?.actions.some(action => action.reason === 'nativeInstallerFailedAfterRemoval')
  ) {
    details.push(t('history.applicationUninstallReasons.nativeInstallerFailedAfterRemoval'));
  }
  if (
    result.results.length === 1 &&
    result.results[0]?.actions.some(action => action.reason === 'removalUnconfirmed')
  ) {
    details.push(t('history.applicationUninstallReasons.removalUnconfirmed'));
  }
  if (
    result.results.length === 1 &&
    result.results[0]?.actions.some(action => action.reason === 'verificationFailed')
  ) {
    details.push(t('history.applicationUninstallReasons.verificationFailed'));
  }
  if (result.restartRequired) details.push(t('applicationUninstall.restartRequired'));
  return {
    title,
    tone: hasFailures
      ? ('warning' as const)
      : cancelledApplications || continuingApplications
        ? ('info' as const)
        : ('success' as const),
    options: { ...(details.length ? { description: details.join(' · ') } : {}), id: UNINSTALL_RESULT_TOAST_ID },
  };
}
