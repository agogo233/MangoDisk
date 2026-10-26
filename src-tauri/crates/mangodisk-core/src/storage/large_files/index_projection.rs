use super::LargeFileEntry;
use crate::{filesystem::metadata::display_path, storage::index::cache::IndexedFile};
use mangodisk_platform::{current_platform, Platform, ScanPurpose};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};
pub(crate) fn large_file_entries_from_snapshot(
    root: &Path,
    files: &HashMap<PathBuf, IndexedFile>,
) -> Vec<LargeFileEntry> {
    let mut entries = files
        .iter()
        .filter(|(path, file)| {
            path.starts_with(root)
                && file.bytes >= crate::storage::large_files::LARGE_FILE_CANDIDATE_FLOOR_BYTES
        })
        .filter(|(path, _)| {
            current_platform()
                .should_skip(path, root, ScanPurpose::LargeFiles)
                .is_none()
        })
        .map(|(path, file)| large_file_entry(path, root, *file))
        .collect::<Vec<_>>();
    entries.sort_by(|left, right| {
        right
            .bytes
            .cmp(&left.bytes)
            .then_with(|| left.path.cmp(&right.path))
    });
    entries
}

fn large_file_entry(path: &Path, root: &Path, file: IndexedFile) -> LargeFileEntry {
    LargeFileEntry {
        name: path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default(),
        path: display_path(path),
        parent_path: display_path(path.parent().unwrap_or(root)),
        bytes: file.bytes,
        logical_bytes: file.logical_bytes,
        modified_at_ms: file.modified_at_ms,
    }
}
