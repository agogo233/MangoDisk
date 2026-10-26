export type GpuActivityKind =
  'graphics' | 'copy' | 'videoDecode' | 'videoEncode' | 'videoProcessing' | 'other' | 'renderer' | 'tiler';
export interface GpuActivity {
  id: string;
  kind: GpuActivityKind;
  name: string | null;
  usedPercent: number;
  includedInSummary: boolean;
}
export interface GpuMemory {
  dedicatedUsedBytes: number | null;
  dedicatedTotalBytes: number | null;
  dedicatedTotalSource: 'reported' | 'allocatable' | null;
  sharedUsedBytes: number | null;
  sharedTotalBytes: number | null;
}
export interface GpuDetails {
  activities: GpuActivity[];
  telemetry: {
    coreCount: number | null;
    temperatureCelsius: number | null;
    engineClockMhz: number | null;
    coreClockMhz: number | null;
    memoryClockMhz: number | null;
    fanPercent: number | null;
  };
  memoryArchitecture: 'unified' | 'dedicated' | 'shared' | 'unknown';
  memoryStatus: 'ready' | 'unsupported' | 'failed';
  memory: GpuMemory | null;
}
export const GPU_ACTIVITY_LABEL_KEYS: Record<GpuActivityKind, string> = {
  graphics: 'gpuDetails.graphics',
  copy: 'gpuDetails.copy',
  videoDecode: 'gpuDetails.videoDecode',
  videoEncode: 'gpuDetails.videoEncode',
  videoProcessing: 'gpuDetails.videoProcessing',
  other: 'gpuDetails.other',
  renderer: 'gpuDetails.renderer',
  tiler: 'gpuDetails.tiler',
};

export const GPU_FACT_LABEL_KEYS = {
  coreCount: 'gpuDetails.coreCount',
  average: 'gpuDetails.average',
  peak: 'gpuDetails.peak',
  temperature: 'gpuDetails.temperature',
  coreClock: 'gpuDetails.coreClock',
  engineClock: 'gpuDetails.engineClock',
  memoryClock: 'gpuDetails.memoryClock',
  fanSpeed: 'gpuDetails.fanSpeed',
} as const;
