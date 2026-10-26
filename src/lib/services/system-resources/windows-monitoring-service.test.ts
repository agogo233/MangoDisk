import { beforeEach, expect, it, vi } from 'vitest';
import { invoke } from '@tauri-apps/api/core';
import { WindowsMonitoringService } from '@/lib/services/system-resources/windows-monitoring-service';
vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }));
beforeEach(() => vi.clearAllMocks());
it('uses only fixed capability command names without transport-supplied paths', async () => {
  await WindowsMonitoringService.observe();
  await WindowsMonitoringService.install();
  await WindowsMonitoringService.uninstall();
  await WindowsMonitoringService.uninstall(true);
  expect(vi.mocked(invoke).mock.calls).toEqual([
    ['windows_monitoring_observe'],
    ['windows_monitoring_install'],
    ['windows_monitoring_uninstall', { removeDriver: false }],
    ['windows_monitoring_uninstall', { removeDriver: true }],
  ]);
});
