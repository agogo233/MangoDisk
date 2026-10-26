import type { GpuDeviceReading, MetricStatus } from '@/lib/models/system-resources/system-resources';
import { currentGpuTemperature } from './gpu-temperature';
import { hasFreshValue } from './metric-reading';

/** Device identity and original timestamps remain authoritative across view changes. */
export function gpuDeviceMetrics(device: GpuDeviceReading, now: number) {
  const usage = device.usage;
  const value = usage.value;
  const usedPercent =
    hasFreshValue(usage, now, 5000) &&
    value?.adapterId === device.adapterId &&
    Number.isFinite(value.usedPercent) &&
    value.usedPercent >= 0 &&
    value.usedPercent <= 100
      ? value.usedPercent
      : null;
  const temperature = currentGpuTemperature(device.temperature, now);
  const celsius = temperature?.adapterId === device.adapterId ? temperature.celsius : null;
  const status = (readingStatus: MetricStatus, valid: boolean): MetricStatus =>
    readingStatus === 'ready' && !valid ? 'stale' : readingStatus;
  return {
    usedPercent,
    celsius,
    usageStatus: status(usage.status, usedPercent !== null),
    temperatureStatus: status(device.temperature.status, celsius !== null),
  };
}

/** Automatic display follows the busiest fresh GPU, with stable catalogue-order ties. */
export function automaticGpuDevice(devices: GpuDeviceReading[], now: number): GpuDeviceReading | undefined {
  let selected: GpuDeviceReading | undefined;
  let maximum = -1;
  for (const device of devices) {
    const usage = gpuDeviceMetrics(device, now).usedPercent;
    if (usage !== null && usage > maximum) {
      selected = device;
      maximum = usage;
    }
  }
  return selected ?? devices[0];
}
