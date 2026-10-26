import { invoke } from '@tauri-apps/api/core';
import { getCurrentWindow } from '@tauri-apps/api/window';
import type { UnlistenFn } from '@tauri-apps/api/event';
import type { AnalysisLaunchRequest, ExplorerIntegrationStatus } from '@/lib/models/settings/explorer-integration';

export class ExplorerIntegrationService {
  static status(): Promise<ExplorerIntegrationStatus> {
    return invoke('explorer_integration_status');
  }

  static setEnabled(enabled: boolean, title: string): Promise<ExplorerIntegrationStatus> {
    return invoke('explorer_integration_set_enabled', { enabled, title });
  }

  static async watchAnalysisLaunch(handler: (path: string) => void): Promise<UnlistenFn> {
    let stopped = false;
    // Register before the initial read: a cold launch remains queued in Rust,
    // while warm launches use the same queue and cannot be delivered twice.
    const receive = async () => {
      const request = await invoke<AnalysisLaunchRequest | null>('take_analysis_launch');
      if (!stopped && request?.schemaVersion === 1) handler(request.path);
    };
    const stop = await getCurrentWindow().listen('analysis-directory-requested', () => {
      void receive().catch(() => undefined);
    });
    try {
      await receive();
    } catch (error) {
      stop();
      throw error;
    }
    return () => {
      stopped = true;
      stop();
    };
  }
}
