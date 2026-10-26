import { describe, expect, it } from 'vitest';
import type { GpuDeviceReading } from '@/lib/models/system-resources/system-resources';
import { automaticGpuDevice, gpuDeviceMetrics } from './gpu-device-metrics';
function device(): GpuDeviceReading {
  return {
    adapterId: 'a',
    adapterName: 'GPU 0',
    usage: {
      status: 'ready',
      sampledAtMs: 1000,
      value: { adapterId: 'a', adapterName: 'a', usedPercent: 0, details: null },
    },
    usageHistory: [],
    temperature: { status: 'unsupported', sampledAtMs: null, value: null },
    temperatureHistory: [],
  };
}
describe('GPU device metrics', () => {
  it('keeps real zero and unsupported temperature separate, and expires original samples', () => {
    expect(gpuDeviceMetrics(device(), 1000)).toEqual({
      usedPercent: 0,
      celsius: null,
      usageStatus: 'ready',
      temperatureStatus: 'unsupported',
    });
    expect(gpuDeviceMetrics(device(), 6001).usageStatus).toBe('stale');
    expect(gpuDeviceMetrics(device(), 999).usedPercent).toBeNull();
  });
  it('rejects mismatched devices and invalid or failed values without replacing their status', () => {
    const sample = device();
    sample.usage.value!.adapterId = 'b';
    expect(gpuDeviceMetrics(sample, 1000).usedPercent).toBeNull();
    sample.usage.value!.adapterId = 'a';
    for (const value of [NaN, Infinity, -1, 101]) {
      sample.usage.value!.usedPercent = value;
      expect(gpuDeviceMetrics(sample, 1000).usedPercent).toBeNull();
    }
    sample.usage.status = 'failed';
    expect(gpuDeviceMetrics(sample, 1000).usageStatus).toBe('failed');
  });
});

it('selects the busiest fresh GPU with stable ties and never invents a valid sample', () => {
  const first = device();
  const second = device();
  second.adapterId = 'b';
  second.usage.value!.adapterId = 'b';
  second.usage.value!.usedPercent = 80;
  expect(automaticGpuDevice([first, second], 1000)).toBe(second);
  first.usage.value!.usedPercent = 80;
  expect(automaticGpuDevice([first, second], 1000)).toBe(first);
  first.usage.status = 'failed';
  expect(automaticGpuDevice([first, second], 1000)).toBe(second);
  second.usage.sampledAtMs = 999;
  expect(automaticGpuDevice([first, second], 6000)).toBe(first);
  expect(gpuDeviceMetrics(first, 6000).usedPercent).toBeNull();
  expect(automaticGpuDevice([], 1000)).toBeUndefined();
});
