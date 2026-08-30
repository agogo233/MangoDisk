import type { ApplicationUninstallInstallerKind } from '@/lib/models/application';

export type ApplicationSizeHintKey =
  'applicationUninstall.windowsAppPackageSizeHint' | 'applicationUninstall.applicationSizeEstimateHint';

/**
 * Windows AppX inventory can include shared package files whose logical size is not attributable
 * to one application. Its dedicated hint explains that limitation; other installer kinds retain
 * the general measured-or-estimated size explanation.
 */
export function applicationSizeHintKey(
  installerKind: ApplicationUninstallInstallerKind | null
): ApplicationSizeHintKey {
  return installerKind === 'windowsAppx'
    ? 'applicationUninstall.windowsAppPackageSizeHint'
    : 'applicationUninstall.applicationSizeEstimateHint';
}
