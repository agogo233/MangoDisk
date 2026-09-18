import { invoke } from '@tauri-apps/api/core';

import type { ApplicationIcon } from '@/lib/models/application-icon';
import type { FileIconRequest } from '@/lib/models/file-icon';
import { FileIconService } from '@/lib/services/file-icon-service';
import { LoggerService } from '@/lib/services/logger-service';

/**
 * Resolves application icons on demand and keeps them for the current session.
 *
 * Cleanup scans intentionally return metadata only. Deferring icon loading
 * until application details are visible keeps scans fast and prevents encoded
 * image data from inflating every scan snapshot.
 */
export class ApplicationIconService {
  // Generic mode uses only the extension to request AppKit's application-bundle
  // icon. This absolute descriptor is never opened or treated as an installed app.
  private static readonly macOsFallbackRequest: FileIconRequest = {
    path: '/.mangodisk-generic-application.app',
    kind: 'file',
    mode: 'generic',
  };
  private static readonly batchSize = 32;
  private static readonly cache = new Map<string, string | null>();
  private static pending: Promise<void> | undefined;

  static peekMacOsFallback(): string | null {
    return FileIconService.peek(ApplicationIconService.macOsFallbackRequest, true) ?? null;
  }

  static resolveMacOsFallback(): Promise<string | null> {
    return FileIconService.resolve(ApplicationIconService.macOsFallbackRequest);
  }

  static async resolve(paths: string[]): Promise<ReadonlyMap<string, string>> {
    return ApplicationIconService.resolveIncrementally(paths, () => undefined);
  }

  /**
   * Publishes every completed native batch instead of waiting for the entire
   * catalog. The page can therefore paint visible icons while lower rows are
   * still being resolved, and the same API remains useful for smaller views.
   */
  static async resolveIncrementally(
    paths: string[],
    onUpdate: (icons: ReadonlyMap<string, string>) => void
  ): Promise<ReadonlyMap<string, string>> {
    const uniquePaths = [...new Set(paths)];
    onUpdate(ApplicationIconService.snapshot(uniquePaths));

    while (ApplicationIconService.hasMissingPaths(uniquePaths)) {
      if (!ApplicationIconService.pending) {
        const missingPaths = uniquePaths
          .filter(path => !ApplicationIconService.cache.has(path))
          .slice(0, ApplicationIconService.batchSize);
        ApplicationIconService.pending = ApplicationIconService.load(missingPaths).finally(() => {
          ApplicationIconService.pending = undefined;
        });
      }
      await ApplicationIconService.pending;
      onUpdate(ApplicationIconService.snapshot(uniquePaths));
    }

    return ApplicationIconService.snapshot(uniquePaths);
  }

  private static snapshot(paths: string[]): ReadonlyMap<string, string> {
    return new Map(
      paths.flatMap(path => {
        const dataUrl = ApplicationIconService.cache.get(path);
        return dataUrl ? [[path, dataUrl] as const] : [];
      })
    );
  }

  private static hasMissingPaths(paths: string[]): boolean {
    return paths.some(path => !ApplicationIconService.cache.has(path));
  }

  private static async load(paths: string[]): Promise<void> {
    try {
      const icons = await invoke<ApplicationIcon[]>('get_application_icons', { paths });
      const iconsByPath = new Map(icons.map(icon => [icon.path, icon.dataUrl]));

      for (const path of paths) {
        ApplicationIconService.cache.set(path, iconsByPath.get(path) ?? null);
      }
    } catch (error) {
      LoggerService.warn('application-icons', 'load-failed', {
        requestedCount: paths.length,
        error: String(error),
      });

      // Cache a missing result for this session so repeatedly opening the same
      // details does not retry a platform capability that has already failed.
      for (const path of paths) {
        ApplicationIconService.cache.set(path, null);
      }
    }
  }
}
