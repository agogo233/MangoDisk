import type { ResourceReadings, MetricReading } from '@/lib/models/system-resources';

export function emptyReadings(): ResourceReadings {
  const empty = <T>(): MetricReading<T> => ({ status: 'loading', sampledAtMs: null, value: null });
  return {
    schemaVersion: 3,
    observedAtMs: 0,
    cpu: empty(),
    memory: empty(),
    network: empty(),
    disk: empty(),
    diskIo: empty(),
    interfaces: [],
    volumes: [],
    cpuHistory: [],
    networkHistory: [],
    memoryHistory: [],
    diskIoHistory: [],
  };
}
