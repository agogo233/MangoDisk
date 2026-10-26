import {
  STANDARD_SCAN_FOLDER_IDS,
  type StandardScanFolder,
  type StandardScanFolderId,
} from '@/lib/models/storage/standard-scan-folder';
import { audioDir, documentDir, downloadDir, pictureDir, videoDir } from '@tauri-apps/api/path';

import { FolderSelectionService } from '@/lib/services/storage/folder-selection-service';
import * as PathUtils from '@/lib/utils/filesystem/path';

interface StandardScanFolderDefinition {
  id: StandardScanFolderId;
  resolvePath: () => Promise<string>;
}

const standardFolderDefinitions: readonly StandardScanFolderDefinition[] = [
  { id: STANDARD_SCAN_FOLDER_IDS.downloads, resolvePath: downloadDir },
  { id: STANDARD_SCAN_FOLDER_IDS.documents, resolvePath: documentDir },
  { id: STANDARD_SCAN_FOLDER_IDS.pictures, resolvePath: pictureDir },
  { id: STANDARD_SCAN_FOLDER_IDS.videos, resolvePath: videoDir },
  { id: STANDARD_SCAN_FOLDER_IDS.music, resolvePath: audioDir },
];

/** Resolves operating-system user folders without assuming localized names or home paths. */
export class StandardScanFolderService {
  static async listAvailable(): Promise<StandardScanFolder[]> {
    const resolved = await Promise.allSettled(
      standardFolderDefinitions.map(async definition => ({
        id: definition.id,
        path: PathUtils.display(await definition.resolvePath()),
      }))
    );
    const candidates = resolved.flatMap(result => (result.status === 'fulfilled' ? [result.value] : []));
    if (candidates.length === 0) return [];

    const existing = await FolderSelectionService.resolveDirectories(candidates.map(candidate => candidate.path));
    const targets = new Map(existing.map(entry => [PathUtils.comparisonKey(entry.requestedPath), entry.path]));
    const seen = new Set<string>();
    return candidates.flatMap(candidate => {
      const target = targets.get(PathUtils.comparisonKey(candidate.path));
      if (!target) return [];
      const key = PathUtils.comparisonKey(target);
      if (seen.has(key)) return [];
      seen.add(key);
      return [{ id: candidate.id, path: target }];
    });
  }
}
