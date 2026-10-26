import type { TrendPoint } from '@/lib/models/system-resources/system-resources';

const WINDOW_MS = 60_000;
const INTERVAL_MS = 2_000;
const MAX_INTERVAL_MS = 5_000;

/** Utilization samples end counter intervals. Never extend a value across an outage. */
export function summarizeUtilizationHistory(history: readonly TrendPoint[], observedAtMs: number) {
  if (!Number.isFinite(observedAtMs)) return null;
  const start = observedAtMs - WINDOW_MS;
  const points = history
    .filter(point => Number.isFinite(point.sampledAtMs) && point.sampledAtMs <= observedAtMs)
    .slice()
    .sort((a, b) => a.sampledAtMs - b.sampledAtMs);
  let previousAt: number | undefined;
  let weighted = 0;
  let coveredMs = 0;
  let peak: number | null = null;
  for (let index = 0; index < points.length; index++) {
    const point = points[index]!;
    // Last observation owns a repeated timestamp, without double-counting time.
    if (points[index + 1]?.sampledAtMs === point.sampledAtMs) continue;
    const elapsed = previousAt === undefined ? INTERVAL_MS : point.sampledAtMs - previousAt;
    const interval = elapsed <= MAX_INTERVAL_MS ? elapsed : INTERVAL_MS;
    previousAt = point.sampledAtMs;
    if (!Number.isFinite(point.primary) || point.primary < 0 || point.primary > 100 || point.sampledAtMs < start)
      continue;
    peak = Math.max(peak ?? point.primary, point.primary);
    const duration = Math.max(0, point.sampledAtMs - Math.max(start, point.sampledAtMs - interval));
    weighted += point.primary * duration;
    coveredMs += duration;
  }
  return coveredMs > 0 && peak !== null ? { average: weighted / coveredMs, peak, coveredMs } : null;
}
