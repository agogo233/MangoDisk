import { invoke } from '@tauri-apps/api/core';
import { getCurrentWindow } from '@tauri-apps/api/window';

import type { VolumeInfo } from '@/lib/models/storage/volume';
import { LOG_DOMAINS, LOG_EVENTS } from '@/lib/models/diagnostics/telemetry';
import { LoggerService } from '@/lib/services/diagnostics/logger-service';

/** Exposes volume enumeration without aggregating unrelated native commands. */
// Command names are retained for compatibility with the existing desktop IPC contract.
export class VolumeService {
  static getSystemVolume(refresh = false): Promise<VolumeInfo> {
    return invoke<VolumeInfo>('get_system_disk', { refresh });
  }

  static listVolumes(): Promise<VolumeInfo[]> {
    return invoke<VolumeInfo[]>('list_disks');
  }

  /** Refresh active windows cheaply; returning to the app always requests a snapshot. */
  static async watchCapacity(refresh: () => Promise<unknown>): Promise<() => void> {
    const current = getCurrentWindow();
    let disposed = false;
    let focused = false;
    let focusObserved = false;
    let pending = false;
    const read = async () => {
      if (disposed || pending) return;
      pending = true;
      try {
        await refresh();
      } catch (error) {
        LoggerService.warn(LOG_DOMAINS.applicationShell, LOG_EVENTS.diskRefreshFailed, { error });
      } finally {
        pending = false;
      }
    };
    const stopFocus = await current.onFocusChanged(event => {
      focusObserved = true;
      focused = event.payload;
      if (focused) void read();
    });
    try {
      const initialFocus = await current.isFocused();
      if (!focusObserved) focused = initialFocus;
    } catch (error) {
      stopFocus();
      throw error;
    }
    const timer = setInterval(() => {
      if (focused) void read();
    }, 15_000);
    return () => {
      disposed = true;
      clearInterval(timer);
      stopFocus();
    };
  }
}
