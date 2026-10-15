import { describe, expect, it } from 'vitest';
import type { GpuMemory } from '@/lib/models/gpu-details';
import { gpuMemoryRows } from './gpu-memory';

const memory: GpuMemory = {
  dedicatedUsedBytes: 20,
  dedicatedTotalBytes: 128,
  dedicatedTotalSource: 'reported',
  sharedUsedBytes: 80,
  sharedTotalBytes: 1024,
};
describe('GPU memory observations', () => {
  it('adds dedicated and shared usage without summing process allocations', () => {
    const rows = gpuMemoryRows(memory);
    expect(rows.map(row => row.key)).toEqual(['total', 'dedicated', 'shared']);
    expect(rows[0]).toMatchObject({ used: 100, total: 1152 });
    expect(rows[1]).toMatchObject({ used: 20, total: 128 });
    expect(rows[2]).toMatchObject({ used: 80, total: 1024 });
  });
  it('retains measured usage when a physical capacity is unavailable', () => {
    const rows = gpuMemoryRows({ ...memory, dedicatedTotalBytes: null });
    expect(rows[0]).toMatchObject({ used: 100, total: null, percent: null });
    expect(rows[1]).toMatchObject({ used: 20, total: null, percent: null });
    expect(rows[2].total).toBe(1024);
  });
  it('does not invent total usage when one component is missing', () => {
    expect(gpuMemoryRows({ ...memory, dedicatedUsedBytes: null }).map(row => row.key)).toEqual(['shared']);
    expect(gpuMemoryRows({ ...memory, sharedUsedBytes: null }).map(row => row.key)).toEqual(['dedicated']);
    expect(gpuMemoryRows(null)).toEqual([]);
  });
  it('distinguishes zero capacity from unknown and bounds the meter', () => {
    const rows = gpuMemoryRows({ ...memory, dedicatedUsedBytes: 0, dedicatedTotalBytes: 0 });
    expect(rows.map(row => row.key)).toEqual(['total', 'shared']);
    expect(rows[0]).toMatchObject({ used: 80, total: 1024 });
    const inconsistent = gpuMemoryRows({ ...memory, dedicatedUsedBytes: 10, dedicatedTotalBytes: 0 });
    expect(inconsistent.find(row => row.key === 'dedicated')).toMatchObject({ used: 10, total: 0, percent: null });
    expect(gpuMemoryRows({ ...memory, sharedUsedBytes: 2048 })[2].percent).toBe(100);
  });
  it('rejects invalid byte values and unsafe sums', () => {
    for (const invalid of [-1, NaN, Infinity, 1.5, Number.MAX_SAFE_INTEGER + 1]) {
      expect(gpuMemoryRows({ ...memory, dedicatedUsedBytes: invalid }).map(row => row.key)).toEqual(['shared']);
    }
    const rows = gpuMemoryRows({
      ...memory,
      dedicatedUsedBytes: Number.MAX_SAFE_INTEGER,
      dedicatedTotalBytes: Number.MAX_SAFE_INTEGER,
    });
    expect(rows.map(row => row.key)).toEqual(['dedicated', 'shared']);
  });
});
