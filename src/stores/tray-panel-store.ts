import { emptyReadings } from '@/lib/utils/system-resources';
import type { MetricId } from '@/lib/models/system-resources';
import { defineStore } from 'pinia';

import type { MemoryReleaseResult, ResidentReading } from '@/lib/models/resident';
import { ResidentService } from '@/lib/services/resident-service';
import { LoggerService } from '@/lib/services/logger-service';

export const useTrayPanelStore = defineStore('tray-panel', {
  state: () => ({
    reading: { revision: 0, ...emptyReadings() } as ResidentReading,
    selectedMetric: 'cpu' as MetricId,
    error: false,
    refreshing: false,
    releasing: false,
    releaseResult: null as MemoryReleaseResult | null,
  }),
  actions: {
    accept(reading: ResidentReading) {
      // A cached IPC response can arrive after a newer native event. Never move
      // backwards or show an incompatible protocol as a plausible measurement.
      if (reading.schemaVersion !== 3 || (reading.memory.value && reading.memory.value.schemaVersion !== 1)) {
        this.error = true;
        return;
      }
      if (reading.revision < this.reading.revision) return;
      this.reading = reading;
      this.error = false;
      this.refreshing = false;
    },
    async load() {
      try {
        this.accept(await ResidentService.reading());
      } catch {
        this.fail('monitoring_read_failed');
      }
    },
    async refresh() {
      if (this.refreshing) return;
      this.refreshing = true;
      try {
        await ResidentService.refresh();
        await this.load();
      } catch {
        this.refreshing = false;
        this.fail('monitoring_refresh_failed');
      } finally {
        this.refreshing = false;
      }
    },
    async releaseMemory() {
      if (this.releasing) return;
      this.releasing = true;
      this.releaseResult = null;
      try {
        const result = await ResidentService.releaseMemory();
        if (result.schemaVersion !== 1) throw new Error('unsupported memory release response');
        this.releaseResult = result;
        await this.refresh();
      } catch {
        this.releaseResult = { schemaVersion: 1, status: 'failed', observedReductionBytes: null };
        LoggerService.warn('monitoring', 'memory_release_request_failed');
      } finally {
        this.releasing = false;
      }
    },
    fail(event: string) {
      this.error = true;
      LoggerService.warn('monitoring', event);
    },
  },
});
