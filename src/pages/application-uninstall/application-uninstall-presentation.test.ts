import { describe, expect, it } from 'vitest';

import type { ApplicationUninstallInstallerKind } from '@/lib/models/application';

import { applicationSizeHintKey } from './application-uninstall-presentation';

describe('applicationSizeHintKey', () => {
  it('uses the shared package explanation for Windows AppX', () => {
    expect(applicationSizeHintKey('windowsAppx')).toBe('applicationUninstall.windowsAppPackageSizeHint');
  });

  it('uses the general estimate explanation for every other installer kind', () => {
    const installerKinds: Array<ApplicationUninstallInstallerKind | null> = [
      null,
      'windowsMsi',
      'windowsScoop',
      'windowsChocolatey',
      'windowsRegistered',
    ];
    for (const installerKind of installerKinds) {
      expect(applicationSizeHintKey(installerKind)).toBe('applicationUninstall.applicationSizeEstimateHint');
    }
  });
});
