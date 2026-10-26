import { describe, expect, it } from 'vitest';
import type { ApplicationUninstallExecutionProgress } from '@/lib/models/applications/application';
import { applicationUninstallExecutionItems } from './application-uninstall-execution';

const plans = [{ applicationId: 'first' }, { applicationId: 'second' }];
const progress: Pick<
  ApplicationUninstallExecutionProgress,
  'stage' | 'currentApplicationId' | 'completedApplications'
> = {
  stage: 'uninstalling',
  currentApplicationId: 'second',
  completedApplications: [{ applicationId: 'first', status: 'completed', releasedBytes: 0 }],
};

describe('application uninstall execution presentation', () => {
  it('retains plan order and resolves names from the catalog before the preview', () => {
    const items = applicationUninstallExecutionItems(
      plans,
      [{ applicationId: 'first', name: 'Catalog name', executionMode: null }],
      [
        { applicationId: 'first', applicationName: 'Preview name' },
        { applicationId: 'second', applicationName: 'Second' },
      ],
      progress
    );
    expect(items.map(item => [item.applicationId, item.name, item.state])).toEqual([
      ['first', 'Catalog name', 'completed'],
      ['second', 'Second', 'active'],
    ]);
  });

  it.each(['completed', 'cancelled', 'failed'] as const)(
    'keeps a %s result terminal even when its ID is current',
    status => {
      const [item] = applicationUninstallExecutionItems([plans[1]!], [], [], {
        ...progress,
        completedApplications: [{ applicationId: 'second', status, releasedBytes: 0 }],
      });
      expect(item?.state).toBe(status);
      expect(item?.statusLabelKey).toBe(`applicationUninstall.uninstallItemStatus.${status}`);
    }
  );

  it('uses interactive guidance only for an active interactive uninstaller', () => {
    const candidates = [{ applicationId: 'second', name: 'Second', executionMode: 'interactive' as const }];
    expect(applicationUninstallExecutionItems(plans, candidates, [], progress)[1]?.detailKey).toBe(
      'applicationUninstall.uninstallItemInteractive'
    );
    expect(
      applicationUninstallExecutionItems(plans, candidates, [], { ...progress, stage: 'finalizing' })[1]?.state
    ).toBe('pending');
    expect(applicationUninstallExecutionItems(plans, [], [], progress)[1]?.detailKey).toBe(
      'applicationUninstall.uninstallItemActive'
    );
  });

  it('leaves unknown names for the locale adapter and starts without an active item', () => {
    const [item] = applicationUninstallExecutionItems(plans, [], [], null);
    expect(item).toMatchObject({
      name: null,
      state: 'pending',
      detailKey: 'applicationUninstall.uninstallItemPending',
    });
    expect(applicationUninstallExecutionItems([], [], [], progress)).toEqual([]);
  });
});
