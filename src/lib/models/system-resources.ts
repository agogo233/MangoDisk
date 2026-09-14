export const METRIC_IDS = ['cpu', 'memory', 'network', 'disk'] as const;
export type MetricId = (typeof METRIC_IDS)[number];
export type MetricStatus = 'loading' | 'ready' | 'stale' | 'disconnected' | 'unsupported' | 'failed';
export const METRIC_LABEL_KEYS: Record<MetricId, string> = {
  cpu: 'systemStatus.cpu',
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
export interface MemoryOverview {
  totalBytes: number;
  usedBytes: number;
  freeBytes: number;
  swapUsedBytes: number;
  usedPercent: number;
}
export interface ApplicationMemory {
  id: string;
  name: string;
  residentBytes: number;
  processCount: number;
  iconPath: string | null;
  isBundle: boolean;
  canQuit: boolean;
}
export interface ProcessMemorySummary {
  applications: ApplicationMemory[];
  readableProcessCount: number;
  omittedProcessCount: number;
}
export interface SystemResourceSnapshot {
  schemaVersion: 1;
  sampledAtMs: number;
  memory: MemoryOverview;
  processes: ProcessMemorySummary | null;
}
export interface CpuUsage {
  usedPercent: number;
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
export interface ResourceReadings {
  schemaVersion: 3;
  observedAtMs: number;
  cpu: MetricReading<CpuUsage>;
  memory: MetricReading<SystemResourceSnapshot>;
  network: MetricReading<NetworkRate>;
  disk: MetricReading<DiskUsage>;
  diskIo: MetricReading<DiskIoRate>;
  interfaces: NetworkInterface[];
  volumes: ResourceVolume[];
  cpuHistory: TrendPoint[];
  networkHistory: TrendPoint[];
  memoryHistory: TrendPoint[];
  diskIoHistory: TrendPoint[];
}
