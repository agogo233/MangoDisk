import { beforeEach, describe, expect, it, vi } from 'vitest';

const invokeMock = vi.hoisted(() => vi.fn());

vi.mock('@tauri-apps/api/core', () => ({ invoke: invokeMock }));
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn() }));

import { AnalysisService } from '@/lib/services/analysis-service';

describe('AnalysisService', () => {
  beforeEach(() => {
    invokeMock.mockReset();
    invokeMock.mockResolvedValue(undefined);
  });

  it('passes the selected analysis exclusions to the native scan', async () => {
    await AnalysisService.analyze('/fixture', false, ['/fixture/cache']);

    expect(invokeMock).toHaveBeenCalledWith('analyze_path', {
      path: '/fixture',
      refresh: false,
      excludedPaths: ['/fixture/cache'],
    });
  });

  it('keeps an unfiltered analysis request explicit', async () => {
    await AnalysisService.analyze(undefined, true, []);

    expect(invokeMock).toHaveBeenCalledWith('analyze_path', {
      path: null,
      refresh: true,
      excludedPaths: [],
    });
  });
});
