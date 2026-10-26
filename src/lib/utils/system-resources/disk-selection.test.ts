import { describe, expect, it } from 'vitest';
import type { ResourceVolume } from '@/lib/models/system-resources/system-resources';
import { diskDevices, diskDeviceVolumes, diskVolumeGroups, selectedDiskVolume } from './disk-selection';

function diskVolumes(): ResourceVolume[] {
  const a = { id: 'ssd-a', number: 0, model: 'Samsung SSD 990 PRO' };
  const b = { id: 'ssd-b', number: 1, model: 'WD Blue SN580' };
  return [
    { id: 'f', name: 'Archive (F:)', system: false, fileSystem: 'NTFS', mountPoints: ['F:'], devices: [b] },
    { id: 'd', name: 'D:', system: false, fileSystem: 'NTFS', mountPoints: ['D:'], devices: [a] },
    { id: 'c', name: 'Windows (C:)', system: true, fileSystem: 'NTFS', mountPoints: ['C:'], devices: [a] },
  ];
}

describe('physical disk presentation', () => {
  it('groups volumes by device identity and sorts letters independently of GUID order', () => {
    const volumes = diskVolumes();
    expect(diskVolumeGroups(volumes).map(group => group.volumes.map(volume => volume.id))).toEqual([['c', 'd'], ['f']]);
    expect(diskDevices(volumes).map(device => device.id)).toEqual(['ssd-a', 'ssd-b']);
    expect(diskDeviceVolumes(volumes[1]!.devices[0]!, volumes)).toEqual(['C:', 'D:']);
    expect(volumes.map(volume => volume.id)).toEqual(['f', 'd', 'c']);
  });
  it('keeps ambiguous and unmapped volumes selectable once without inventing physical ownership', () => {
    const volumes = diskVolumes();
    const span = { ...volumes[0]!, id: 'span', devices: diskDevices(volumes) };
    const unknown = { ...volumes[0]!, id: 'unknown', devices: [] };
    const groups = diskVolumeGroups([...volumes, span, unknown]);
    expect(
      groups
        .flatMap(group => group.volumes)
        .map(volume => volume.id)
        .sort()
    ).toEqual(['c', 'd', 'f', 'span', 'unknown']);
    expect(groups.find(group => group.volumes[0]?.id === 'span')?.devices).toHaveLength(2);
    expect(groups.find(group => group.volumes[0]?.id === 'unknown')?.devices).toHaveLength(0);
  });
  it('uses macOS volume names and never substitutes a new volume for a missing saved identity', () => {
    const volumes = diskVolumes().map(volume => ({ ...volume, mountPoints: ['/Volumes/data'] }));
    expect(diskDeviceVolumes(volumes[0]!.devices[0]!, volumes)).toEqual(['Archive (F:)']);
    expect(selectedDiskVolume(volumes, null)?.id).toBe('c');
    expect(selectedDiskVolume(volumes, 'removed')).toBeUndefined();
  });
});
