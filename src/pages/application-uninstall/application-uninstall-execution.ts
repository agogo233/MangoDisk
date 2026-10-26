import type {
  ApplicationUninstallCandidate,
  ApplicationUninstallExecutionProgress,
  ApplicationUninstallPlan,
  ApplicationUninstallResult,
} from '@/lib/models/applications/application';

const STATUS_LABEL_KEYS = {
  completed: 'applicationUninstall.uninstallItemStatus.completed',
  cancelled: 'applicationUninstall.uninstallItemStatus.cancelled',
  failed: 'applicationUninstall.uninstallItemStatus.failed',
  active: 'applicationUninstall.uninstallItemStatus.active',
  pending: 'applicationUninstall.uninstallItemStatus.pending',
} as const;

export function applicationUninstallExecutionItems(
  plans: readonly Pick<ApplicationUninstallPlan, 'applicationId'>[],
  candidates: readonly Pick<ApplicationUninstallCandidate, 'applicationId' | 'name' | 'executionMode'>[],
  previewResults: readonly Pick<ApplicationUninstallResult, 'applicationId' | 'applicationName'>[],
  progress: Pick<
    ApplicationUninstallExecutionProgress,
    'stage' | 'currentApplicationId' | 'completedApplications'
  > | null
) {
  const completed = new Map((progress?.completedApplications ?? []).map(result => [result.applicationId, result]));
  return plans.map(plan => {
    const candidate = candidates.find(item => item.applicationId === plan.applicationId);
    const preview = previewResults.find(item => item.applicationId === plan.applicationId);
    const result = completed.get(plan.applicationId);
    const active =
      !result && progress?.stage === 'uninstalling' && progress.currentApplicationId === plan.applicationId;
    const state = result?.status ?? (active ? 'active' : 'pending');
    const detailKey =
      state === 'completed'
        ? 'applicationUninstall.uninstallItemCompleted'
        : state === 'cancelled'
          ? 'applicationUninstall.uninstallItemCancelled'
          : state === 'failed'
            ? 'applicationUninstall.uninstallItemFailed'
            : state === 'active'
              ? candidate?.executionMode === 'interactive'
                ? 'applicationUninstall.uninstallItemInteractive'
                : 'applicationUninstall.uninstallItemActive'
              : 'applicationUninstall.uninstallItemPending';
    return {
      applicationId: plan.applicationId,
      name: candidate?.name ?? preview?.applicationName ?? null,
      state,
      detailKey,
      statusLabelKey: STATUS_LABEL_KEYS[state],
    };
  });
}
