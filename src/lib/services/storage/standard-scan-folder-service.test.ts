import { beforeEach, describe, expect, it, vi } from 'vitest';

const pathMocks = vi.hoisted(() => ({
  audioDir: vi.fn(),
  documentDir: vi.fn(),
  downloadDir: vi.fn(),
  pictureDir: vi.fn(),
  videoDir: vi.fn(),
  resolveDirectories: vi.fn(),
}));

vi.mock('@tauri-apps/api/path', () => ({
  audioDir: pathMocks.audioDir,
  documentDir: pathMocks.documentDir,
  downloadDir: pathMocks.downloadDir,
  pictureDir: pathMocks.pictureDir,
  videoDir: pathMocks.videoDir,
}));

vi.mock('@/lib/services/storage/folder-selection-service', () => ({
  FolderSelectionService: {
    resolveDirectories: pathMocks.resolveDirectories,
  },
}));

import { STANDARD_SCAN_FOLDER_IDS } from '@/lib/models/storage/standard-scan-folder';
import { StandardScanFolderService } from '@/lib/services/storage/standard-scan-folder-service';

describe('StandardScanFolderService', () => {
  beforeEach(() => {
    pathMocks.downloadDir.mockResolvedValue('/Users/test/Downloads');
    pathMocks.documentDir.mockResolvedValue('/Users/test/Documents');
    pathMocks.pictureDir.mockResolvedValue('/Users/test/Pictures');
    pathMocks.videoDir.mockResolvedValue('/Users/test/Movies');
    pathMocks.audioDir.mockResolvedValue('/Users/test/Music');
    pathMocks.resolveDirectories.mockImplementation(async (paths: string[]) =>
      paths.map(path => ({ requestedPath: path, path }))
    );
  });

  it('returns standard folders in a stable product order', async () => {
    const folders = await StandardScanFolderService.listAvailable();

    expect(folders.map(folder => folder.id)).toEqual([
      STANDARD_SCAN_FOLDER_IDS.downloads,
      STANDARD_SCAN_FOLDER_IDS.documents,
      STANDARD_SCAN_FOLDER_IDS.pictures,
      STANDARD_SCAN_FOLDER_IDS.videos,
      STANDARD_SCAN_FOLDER_IDS.music,
    ]);
  });

  it('omits unresolved or missing operating-system folders', async () => {
    pathMocks.pictureDir.mockRejectedValue(new Error('picture directory unavailable'));
    pathMocks.resolveDirectories.mockImplementation(async (paths: string[]) =>
      paths.filter(path => !path.endsWith('/Movies')).map(path => ({ requestedPath: path, path }))
    );

    const folders = await StandardScanFolderService.listAvailable();

    expect(folders.map(folder => folder.id)).toEqual([
      STANDARD_SCAN_FOLDER_IDS.downloads,
      STANDARD_SCAN_FOLDER_IDS.documents,
      STANDARD_SCAN_FOLDER_IDS.music,
    ]);
  });

  it('preserves labels when Windows folders resolve to shared targets', async () => {
    pathMocks.documentDir.mockResolvedValue('C:\\Mac\\Home\\Documents');
    pathMocks.pictureDir.mockResolvedValue('C:\\Mac\\Home\\Pictures');
    pathMocks.resolveDirectories.mockImplementation(async (paths: string[]) =>
      paths.map(path => ({
        requestedPath: path,
        path: path.replace('C:\\Mac\\Home', '\\\\psf\\Home'),
      }))
    );
    const folders = await StandardScanFolderService.listAvailable();
    expect(folders).toContainEqual({ id: 'documents', path: '\\\\psf\\Home\\Documents' });
    expect(folders).toContainEqual({ id: 'pictures', path: '\\\\psf\\Home\\Pictures' });
  });

  it('does not render duplicate select values when Known Folders share a target', async () => {
    pathMocks.resolveDirectories.mockImplementation(async (paths: string[]) =>
      paths.map(path => ({
        requestedPath: path,
        path: '\\\\server\\share\\folder',
      }))
    );
    expect(await StandardScanFolderService.listAvailable()).toEqual([
      { id: 'downloads', path: '\\\\server\\share\\folder' },
    ]);
  });
});
