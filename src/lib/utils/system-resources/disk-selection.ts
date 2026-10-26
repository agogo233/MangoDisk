import type { ResourceDisk, ResourceVolume } from '@/lib/models/system-resources/system-resources';

export interface DiskVolumeGroup {
  id: string;
  devices: ResourceDisk[];
  volumes: ResourceVolume[];
}

/** Keep spanning volumes in their own group without duplicating selectable volumes. */
export function diskVolumeGroups(volumes: readonly ResourceVolume[]): DiskVolumeGroup[] {
  const groups = new Map<string, DiskVolumeGroup>();
  for (const volume of volumes) {
    const devices = [...volume.devices].sort(compareDisks);
    const id = JSON.stringify(devices.map(device => device.id).sort());
    const group = groups.get(id) ?? { id, devices, volumes: [] };
    group.volumes.push(volume);
    groups.set(id, group);
  }
  return [...groups.values()]
    .map(group => ({
      ...group,
      volumes: [...group.volumes].sort((a, b) =>
        (a.mountPoints[0] ?? a.name).localeCompare(b.mountPoints[0] ?? b.name, 'en', { numeric: true })
      ),
    }))
    .sort(
      (a, b) =>
        Number(b.volumes.some(volume => volume.system)) - Number(a.volumes.some(volume => volume.system)) ||
        (a.devices[0]?.number ?? Number.MAX_SAFE_INTEGER) - (b.devices[0]?.number ?? Number.MAX_SAFE_INTEGER) ||
        a.id.localeCompare(b.id, 'en')
    );
}

function compareDisks(a: ResourceDisk, b: ResourceDisk): number {
  return (
    (a.number ?? Number.MAX_SAFE_INTEGER) - (b.number ?? Number.MAX_SAFE_INTEGER) || a.id.localeCompare(b.id, 'en')
  );
}

export function diskDevices(volumes: readonly ResourceVolume[]): ResourceDisk[] {
  return [...new Map(volumes.flatMap(volume => volume.devices).map(device => [device.id, device])).values()].sort(
    compareDisks
  );
}

/** Windows shows drive letters/mount folders; macOS shows the mounted volume names. */
export function diskDeviceVolumes(device: ResourceDisk, volumes: readonly ResourceVolume[]): string[] {
  return [
    ...new Set(
      volumes
        .filter(volume => volume.devices.some(item => item.id === device.id))
        .flatMap(volume =>
          volume.mountPoints.filter(path => /^[a-z]:/i.test(path)).length ? volume.mountPoints : [volume.name]
        )
    ),
  ].sort((a, b) => a.localeCompare(b, 'en', { numeric: true }));
}

export function selectedDiskVolume(volumes: readonly ResourceVolume[], id: string | null): ResourceVolume | undefined {
  return volumes.find(volume => (id === null ? volume.system : volume.id === id));
}
