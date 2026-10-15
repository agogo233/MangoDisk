import type { GpuMemory } from '@/lib/models/gpu-details';

function validBytes(value: number | null): number | null {
  return value !== null && Number.isSafeInteger(value) && value >= 0 ? value : null;
}
function sumBytes(left: number | null, right: number | null): number | null {
  return left !== null && right !== null ? validBytes(left + right) : null;
}

/** Totals require both components from the same adapter observation. */
export function gpuMemoryRows(memory: GpuMemory | null) {
  if (!memory) return [];
  const dedicated = validBytes(memory.dedicatedUsedBytes);
  const shared = validBytes(memory.sharedUsedBytes);
  const dedicatedTotal = validBytes(memory.dedicatedTotalBytes);
  const sharedTotal = validBytes(memory.sharedTotalBytes);
  return [
    {
      key: 'total',
      label: 'gpuDetails.totalMemory',
      used: sumBytes(dedicated, shared),
      total: sumBytes(dedicatedTotal, sharedTotal),
    },
    { key: 'dedicated', label: 'gpuDetails.dedicatedMemory', used: dedicated, total: dedicatedTotal },
    { key: 'shared', label: 'gpuDetails.sharedMemory', used: shared, total: sharedTotal },
  ].flatMap(row =>
    row.used === null || (row.key === 'dedicated' && row.used === 0 && row.total === 0)
      ? []
      : [
          {
            ...row,
            used: row.used,
            percent: row.total !== null && row.total > 0 ? Math.min(100, (row.used / row.total) * 100) : null,
          },
        ]
  );
}
