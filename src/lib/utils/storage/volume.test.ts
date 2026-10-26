import { describe, expect, it } from 'vitest';

import type { VolumeInfo } from '@/lib/models/storage/volume';

import * as VolumeUtils from '@/lib/utils/storage/volume';

const systemDisk: VolumeInfo = {
  name: 'System',
  mountPoint: '/',
  totalBytes: 1_000,
  availableBytes: 500,
  usedBytes: 500,
};
const dataDisk: VolumeInfo = {
  name: 'Data',
  mountPoint: '/Volumes/Data',
  totalBytes: 2_000,
  availableBytes: 1_500,
  usedBytes: 500,
};

describe('disk utilities', () => {
  it('prefers the most specific mount and respects path boundaries', () => {
    expect(VolumeUtils.findForPath([systemDisk, dataDisk], '/Volumes/Data/projects')).toBe(dataDisk);
    expect(VolumeUtils.findForPath([dataDisk], '/Volumes/Database', systemDisk)).toBe(systemDisk);
  });

  it('returns the explicit fallback for an empty path', () => {
    expect(VolumeUtils.findForPath([dataDisk], '', systemDisk)).toBe(systemDisk);
  });
});
