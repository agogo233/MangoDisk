import type { GpuDetails } from '@/lib/models/gpu-details';

export const METRIC_IDS = ['cpu', 'gpu', 'memory', 'network', 'disk'] as const;
export type MetricId = (typeof METRIC_IDS)[number];
export type MetricStatus = 'loading' | 'ready' | 'stale' | 'disconnected' | 'unsupported' | 'failed';
export const METRIC_LABEL_KEYS: Record<MetricId, string> = {
  cpu: 'systemStatus.cpu',
  gpu: 'systemStatus.gpu',
  memory: 'systemStatus.memory',
  network: 'systemStatus.network',
  disk: 'systemStatus.disk',
};
export const METRIC_STATUS_KEYS: Record<MetricStatus, string> = {
  loading: 'systemStatus.loading',
  ready: 'systemStatus.ready',
  stale: 'systemStatus.stale',
  disconnected: 'systemStatus.disconnected',
  unsupported: 'systemStatus.unsupported',
  failed: 'systemStatus.failed',
};
export interface MetricReading<T> {
  status: MetricStatus;
  sampledAtMs: number | null;
  value: T | null;
}
export type MemoryPressure = 'unsupported' | 'unavailable' | 'normal' | 'warning' | 'critical';
export interface MemoryOverview {
  totalBytes: number;
  usedBytes: number;
  freeBytes: number;
  swapUsedBytes: number;
  pressure: MemoryPressure;
  usedPercent: number;
}
export interface ApplicationIdentity {
  id: string;
  name: string;
  processCount: number;
  iconPath: string | null;
  isBundle: boolean;
  canQuit: boolean;
}
export interface ApplicationMemory extends ApplicationIdentity {
  usedBytes: number | null;
  readableProcessCount: number;
}
export interface ApplicationCpu extends ApplicationIdentity {
  processes?: { pid: number; startedAt: number; usedPercent: number }[];
  locationStatus?: 'available' | 'denied' | 'exited' | 'unavailable';
  pid: number;
  usedPercent: number;
}
export interface ProcessCpuSummary {
  usageScale: 'singleCore' | 'totalCapacity';
  applications: ApplicationCpu[];
  readableProcessCount: number;
  omittedProcessCount: number;
}
export interface ProcessMemorySummary {
  usageKind: 'physicalFootprint' | 'privateWorkingSet' | 'residentSet';
  applications: ApplicationMemory[];
  readableProcessCount: number;
  omittedProcessCount: number;
}
export interface SystemResourceSnapshot {
  schemaVersion: 4;
  sampledAtMs: number;
  memory: MemoryOverview;
  processes: ProcessMemorySummary | null;
}
export interface CpuUsage {
  usedPercent: number;
}
export interface CpuIdentity {
  model: string | null;
  nominalFrequencyMhz: number | null;
}
export interface CpuFrequency {
  averageMhz: number | null;
  efficiencyMhz: number | null;
  performanceMhz: number | null;
  source: 'windowsPerformance' | 'applePerformanceStates';
}
export const CPU_TEMPERATURE_FRESHNESS_MS = 10_000;
export interface CpuTemperature {
  celsius: number;
  kind: 'coreAverage' | 'package' | 'coreMaximum' | 'diode' | 'proximity';
  source: 'appleSmc' | 'linuxHwmon' | 'intelDtsPawnIo' | 'amdZenPawnIo';
  sensorCount: number;
}
export const GPU_TEMPERATURE_FRESHNESS_MS = 10_000;
export interface GpuTemperature {
  adapterId: string;
  adapterName: string;
  celsius: number;
  kind: 'sensorAverage' | 'device';
  source: 'appleSmc' | 'driver';
  sensorCount: number;
}
export interface GpuUsage {
  usedPercent: number;
  adapterId: string;
  adapterName: string;
  details: GpuDetails | null;
}
export interface NetworkInterface {
  id: string;
  name: string;
  kind: 'ethernet' | 'wifi' | 'virtual' | 'other';
  connected: boolean;
  physical: boolean;
  defaultRouteMetric: number | null;
}
export interface NetworkRate {
  interface: NetworkInterface;
  receivedBytesPerSecond: number;
  transmittedBytesPerSecond: number;
  selectionReason: 'manual' | 'defaultRoute' | 'physicalFallback' | 'disconnected';
}
export interface ResourceVolume {
  id: string;
  name: string;
  system: boolean;
}
export interface DiskUsage {
  volume: ResourceVolume;
  totalBytes: number;
  usedBytes: number;
  availableBytes: number;
  usedPercent: number;
}
export interface TrendPoint {
  sampledAtMs: number;
  primary: number;
  secondary: number | null;
}
export interface DiskIoRate {
  readBytesPerSecond: number;
  writtenBytesPerSecond: number;
}
export type BatterySource = 'appleSmc' | 'appleSmartBattery';
export interface BatteryTemperature {
  celsius: number;
  source: BatterySource;
}
export interface ChargingPower {
  watts: number;
  charging: boolean;
  externalConnected: boolean;
  source: BatterySource;
}
export type BatteryPowerState = 'charging' | 'discharging' | 'idle';
export interface BatteryPower {
  watts: number;
  state: BatteryPowerState;
  source: BatterySource;
}
export interface SystemPower {
  watts: number;
}
export interface DiskTemperature {
  volumeId: string;
  volumeName: string;
  celsius: number;
  source: 'nvmeSmart' | 'ataSmart' | 'windowsStorage';
}

export interface ResourceReadings {
  schemaVersion: 22;
  observedAtMs: number;
  cpu: MetricReading<CpuUsage>;
  cpuIdentity: CpuIdentity | null;
  cpuFrequency: MetricReading<CpuFrequency>;
  cpuTemperature: MetricReading<CpuTemperature>;
  cpuTemperatureHistory: TrendPoint[];
  gpuTemperature: MetricReading<GpuTemperature>;
  gpuTemperatureHistory: TrendPoint[];
  batteryPresent: boolean | null;
  batteryTemperature: MetricReading<BatteryTemperature>;
  chargingPower: MetricReading<ChargingPower>;
  batteryPower: MetricReading<BatteryPower>;
  systemPower: MetricReading<SystemPower>;
  gpu: MetricReading<GpuUsage>;
  gpuDetails: MetricReading<GpuUsage>;
  gpuDetailHistory: TrendPoint[];
  gpuDetailAdapterId: string | null;
  gpuRendererHistory: TrendPoint[];
  gpuTilerHistory: TrendPoint[];
  cpuProcesses: MetricReading<ProcessCpuSummary>;
  memoryProcesses: MetricReading<ProcessMemorySummary>;
  memory: MetricReading<SystemResourceSnapshot>;
  network: MetricReading<NetworkRate>;
  disk: MetricReading<DiskUsage>;
  diskTemperature: MetricReading<DiskTemperature>;
  diskIo: MetricReading<DiskIoRate>;
  interfaces: NetworkInterface[];
  volumes: ResourceVolume[];
  gpuAdapters: { id: string; name: string }[];
  cpuHistory: TrendPoint[];
  gpuHistory: TrendPoint[];
  networkHistory: TrendPoint[];
  memoryHistory: TrendPoint[];
  diskIoHistory: TrendPoint[];
}
