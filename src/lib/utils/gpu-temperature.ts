import { GPU_TEMPERATURE_FRESHNESS_MS, type GpuTemperature, type MetricReading } from '@/lib/models/system-resources';

/** Require a fresh, identified observation; a missing reading never means zero. */
export function currentGpuTemperature(reading: MetricReading<GpuTemperature>, now: number): GpuTemperature | null {
  const value = reading.value;
  const sampled = reading.sampledAtMs;
  return reading.status === 'ready' &&
    sampled !== null &&
    Number.isFinite(sampled) &&
    Number.isFinite(now) &&
    now >= sampled &&
    now - sampled <= GPU_TEMPERATURE_FRESHNESS_MS &&
    value !== null &&
    Number.isFinite(value.celsius) &&
    value.celsius > 0 &&
    value.celsius <= 150 &&
    value.celsius !== 128 &&
    value.adapterId.length > 0 &&
    Number.isInteger(value.sensorCount) &&
    value.sensorCount > 0
    ? value
    : null;
}
