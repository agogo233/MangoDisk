import { describe, expect, it } from 'vitest';
import { STANDARD_SCAN_FOLDER_IDS } from '@/lib/models/storage/standard-scan-folder';
import { findStandardScanFolderByPath } from '@/lib/utils/storage/standard-scan-folder';
describe('standard scan folder path matching', () => {
  it('matches a selected standard folder by its physical path', () => {
    const folders = [
      { id: STANDARD_SCAN_FOLDER_IDS.downloads, path: '/Users/test/Downloads' },
      { id: STANDARD_SCAN_FOLDER_IDS.documents, path: '/Users/test/Documents' },
    ];

    expect(findStandardScanFolderByPath(folders, '/Users/test/Downloads')?.id).toBe(STANDARD_SCAN_FOLDER_IDS.downloads);
    expect(findStandardScanFolderByPath(folders, '/Users/test/Projects')).toBeNull();
  });

  it('uses platform-aware comparison for Windows standard folders', () => {
    const folders = [{ id: STANDARD_SCAN_FOLDER_IDS.downloads, path: 'C:\\Users\\Test\\Downloads' }];

    expect(findStandardScanFolderByPath(folders, 'c:/users/test/downloads/')?.id).toBe(
      STANDARD_SCAN_FOLDER_IDS.downloads
    );
  });
});
