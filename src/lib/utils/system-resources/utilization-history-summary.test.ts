import { describe, expect, it } from 'vitest';
import { summarizeUtilizationHistory } from './utilization-history-summary';
const points = (values: number[][]) =>
  values.map(([sampledAtMs, primary]) => ({ sampledAtMs: sampledAtMs!, primary: primary!, secondary: null }));

describe('utilization history statistics', () => {
  it('weights actual counter intervals instead of the number of observations', () => {
    expect(
      summarizeUtilizationHistory(
        points([
          [2000, 0],
          [3000, 100],
          [6000, 50],
        ]),
        6000
      )
    ).toEqual({
      average: 250 / 6,
      peak: 100,
      coveredMs: 6000,
    });
  });
  it('excludes outages, invalid values and future observations without inventing idle time', () => {
    expect(
      summarizeUtilizationHistory(
        points([
          [9999, 100],
          [10000, 0],
          [30000, 30],
          [70000, 60],
          [70001, 100],
          [50000, NaN],
          [50001, -1],
          [50002, 101],
        ]),
        70000
      )
    ).toEqual({ average: 45, peak: 60, coveredMs: 4000 });
  });
  it('clips the first interval to the minute and does not extend the last value to now', () => {
    expect(
      summarizeUtilizationHistory(
        points([
          [9000, 0],
          [11000, 100],
          [13000, 0],
        ]),
        70000
      )
    ).toEqual({
      average: 100 / 3,
      peak: 100,
      coveredMs: 3000,
    });
  });
  it('handles unordered and repeated timestamps without mutating history or counting intervals twice', () => {
    const history = points([
      [4000, 50],
      [2000, 0],
      [4000, 100],
      [NaN, 100],
    ]);
    const original = structuredClone(history);
    expect(summarizeUtilizationHistory(history, 4000)).toEqual({ average: 50, peak: 100, coveredMs: 4000 });
    expect(history).toEqual(original);
  });
  it('does not manufacture values for empty, expired or zero-coverage history', () => {
    expect(summarizeUtilizationHistory([], 100000)).toBeNull();
    expect(summarizeUtilizationHistory(points([[0, 50]]), 100000)).toBeNull();
    expect(summarizeUtilizationHistory(points([[40000, 50]]), 100000)).toBeNull();
    expect(summarizeUtilizationHistory(points([[1, 50]]), NaN)).toBeNull();
  });
});
