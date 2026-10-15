import { invoke } from '@tauri-apps/api/core';
import type { WindowsMonitoringObservation, WindowsMonitoringSetupResult } from '@/lib/models/windows-monitoring';

export class WindowsMonitoringService {
  static observe(): Promise<WindowsMonitoringObservation> {
    return invoke('windows_monitoring_observe');
  }
  static install(): Promise<WindowsMonitoringSetupResult> {
    return invoke('windows_monitoring_install');
  }
  static uninstall(removeDriver = false): Promise<WindowsMonitoringSetupResult> {
    return invoke('windows_monitoring_uninstall', { removeDriver });
  }
}
