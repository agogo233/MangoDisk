import type { MetricColorRule } from '@/lib/models/resident-color-rules';
import type { MetricId, ResourceReadings } from '@/lib/models/system-resources';

export interface ResidentReading extends ResourceReadings {
  revision: number;
}
export type ResidentDisplayMetricId =
  | MetricId
  | 'cpuTemperature'
  | 'gpuTemperature'
  | 'diskTemperature'
  | 'batteryTemperature'
  | 'chargingPower'
  | 'batteryPower'
  | 'systemPower';

export interface ResidentPreferences {
  schemaVersion: 22;
  windowsCpuTemperatureEnabled: boolean;
  revision: number;
  enabled: boolean;
  showIcon: boolean;
  taskbarPosition: 'auto' | 'left' | 'right';
  taskbarBackground: boolean;
  taskbarCompact: boolean;
  menuBarCompact: boolean;
  usageColors: boolean;
  colorRules: MetricColorRule[];
  metrics: { id: ResidentDisplayMetricId; enabled: boolean }[];
  networkInterface: string | null;
  diskVolume: string | null;
  gpuAdapter: string | null;
}

export type ResidentDestination = 'main' | 'cleanup' | 'applications' | 'settings' | 'about';

export interface MemoryReleaseResult {
  schemaVersion: 1;
  status: 'completed' | 'cancelled' | 'unsupported' | 'failed' | 'busy';
  observedReductionBytes: number | null;
}

export type ApplicationQuitStatus = 'requested' | 'unavailable' | 'unsupported';

export type ResidentDisplayStatus = 'tray' | 'taskbar' | 'noSpace' | 'unsupportedLayout' | 'shellUnavailable';
