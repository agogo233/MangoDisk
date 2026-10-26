use super::{privacy_io_error, MAX_DETAIL_OFFSET, MAX_DETAIL_PAGE_SIZE};
use crate::privacy::{PrivacyDetailEntry, PrivacyDetailsPresentation};
use crate::{CoreError, CoreResult};
use mangodisk_platform::{current_platform, Platform, PlatformPrivacySystemTraceKind};
use std::{fs, io::BufRead, io::BufReader, path::Path, path::PathBuf};

const MAX_DETAIL_LABEL_CHARS: usize = 2_048;
const MAX_DETAIL_LIST_ENTRIES: usize =
    MAX_DETAIL_OFFSET as usize + MAX_DETAIL_PAGE_SIZE as usize + 1;

/// Lists exactly the same root or direct-child units used by the bounded aggregate scan.
///
/// Detail requests are explicit local user actions, so an available full path is more useful than
/// an ambiguous leaf name. Paths are never logged or persisted and remain bounded before crossing
/// the local adapter boundary.
pub(super) fn root_detail_entries(
    roots: &[PathBuf],
    offset: u64,
    limit: u32,
) -> CoreResult<Vec<PrivacyDetailEntry>> {
    root_detail_entries_matching(roots, offset, limit, |_| true)
}

/// Uses the same logical units as platform discovery for file-backed system traces.
///
/// Windows Recent Items and Jump Lists may contain housekeeping files beside the actual privacy
/// records. Filtering those entries keeps the detail count aligned with the aggregate shown to the
/// user instead of making the preview appear larger than the scan result.
pub(super) fn system_root_detail_entries(
    kind: PlatformPrivacySystemTraceKind,
    roots: &[PathBuf],
    offset: u64,
    limit: u32,
) -> CoreResult<Vec<PrivacyDetailEntry>> {
    match kind {
        PlatformPrivacySystemTraceKind::ShellHistory => {
            text_record_detail_entries(roots, offset, limit)
        }
        PlatformPrivacySystemTraceKind::RecentItems
        | PlatformPrivacySystemTraceKind::RecentDocumentHistory => {
            root_detail_entries_matching(roots, offset, limit, |path| {
                path.extension()
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("lnk"))
            })
        }
        PlatformPrivacySystemTraceKind::JumpLists => {
            root_detail_entries_matching(roots, offset, limit, |path| {
                path.extension().is_some_and(|extension| {
                    extension.eq_ignore_ascii_case("automaticDestinations-ms")
                        || extension.eq_ignore_ascii_case("customDestinations-ms")
                })
            })
        }
        _ => root_detail_entries(roots, offset, limit),
    }
}

fn root_detail_entries_matching(
    roots: &[PathBuf],
    offset: u64,
    limit: u32,
    include: impl Fn(&Path) -> bool,
) -> CoreResult<Vec<PrivacyDetailEntry>> {
    let mut entries = Vec::new();
    for root in roots {
        let metadata = fs::symlink_metadata(root)
            .map_err(|error| privacy_io_error("privacy detail root is unavailable", error))?;
        if current_platform().is_link_like(&metadata) {
            return Err(CoreError::operation_failed(
                "privacy detail root is link-like",
            ));
        }
        if metadata.is_dir() {
            for entry in fs::read_dir(root).map_err(|error| {
                privacy_io_error("privacy detail directory is unreadable", error)
            })? {
                let entry = entry.map_err(|error| {
                    privacy_io_error("privacy detail entry is unreadable", error)
                })?;
                let path = entry.path();
                let metadata = fs::symlink_metadata(&path).map_err(|error| {
                    privacy_io_error("privacy detail entry metadata is unavailable", error)
                })?;
                if current_platform().is_link_like(&metadata) {
                    return Err(CoreError::operation_failed(
                        "privacy detail directory contains a link-like entry",
                    ));
                }
                if !include(&path) {
                    continue;
                }
                if entries.len() >= MAX_DETAIL_LIST_ENTRIES {
                    return Err(CoreError::operation_failed(
                        "privacy detail source exceeds the bounded list limit",
                    ));
                }
                entries.push(PrivacyDetailEntry {
                    label: path_detail_label(&path),
                    item_count: 1,
                });
            }
        } else if metadata.is_file() && include(root) {
            if entries.len() >= MAX_DETAIL_LIST_ENTRIES {
                return Err(CoreError::operation_failed(
                    "privacy detail source exceeds the bounded list limit",
                ));
            }
            entries.push(PrivacyDetailEntry {
                label: path_detail_label(root),
                item_count: 1,
            });
        }
    }
    entries.sort_by(|left, right| left.label.cmp(&right.label));
    Ok(page_entries(entries, offset, limit))
}

/// Shell histories are counted by logical text records rather than files, so their preview follows
/// the same unit. Lines are held only for this call, bounded by the same 100k-record scan limit,
/// sanitized for UI rendering, and never logged.
pub(super) fn text_record_detail_entries(
    roots: &[PathBuf],
    offset: u64,
    limit: u32,
) -> CoreResult<Vec<PrivacyDetailEntry>> {
    let start = usize::try_from(offset).unwrap_or(usize::MAX);
    let take = limit as usize;
    let mut seen = 0_usize;
    let mut entries = Vec::new();
    for root in roots {
        let metadata = fs::symlink_metadata(root)
            .map_err(|error| privacy_io_error("privacy text detail is unavailable", error))?;
        if !metadata.is_file() || current_platform().is_link_like(&metadata) {
            return Err(CoreError::operation_failed(
                "privacy text detail source is unsafe",
            ));
        }
        let file = fs::File::open(root)
            .map_err(|error| privacy_io_error("privacy text detail is unreadable", error))?;
        let source_label = history_source_label(root);
        let mut reader = BufReader::new(file);
        let mut record = Vec::new();
        for _ in 0..MAX_DETAIL_OFFSET {
            record.clear();
            let bytes_read = reader.read_until(b'\n', &mut record).map_err(|error| {
                privacy_io_error("privacy text detail record is unreadable", error)
            })?;
            if bytes_read == 0 {
                break;
            }
            if seen >= start && entries.len() < take {
                entries.push(PrivacyDetailEntry {
                    label: history_record_label(source_label, &record),
                    item_count: 1,
                });
            }
            seen = seen.saturating_add(1);
            if entries.len() >= take {
                return Ok(entries);
            }
        }
    }
    Ok(entries)
}

fn history_source_label(path: &Path) -> &'static str {
    match path.file_name().and_then(|name| name.to_str()) {
        Some(".zsh_history") => "zsh",
        Some(".bash_history") => "Bash",
        Some(".sh_history") => "Shell",
        Some(".python_history") => "Python",
        Some(".node_repl_history") => "Node.js",
        Some(".psql_history") => "PostgreSQL",
        Some(".mysql_history") => "MySQL",
        Some(".sqlite_history") => "SQLite",
        Some("ConsoleHost_history.txt") => "PowerShell",
        _ => "Terminal",
    }
}

fn history_record_label(source: &str, record: &[u8]) -> String {
    let record = record.strip_suffix(b"\n").unwrap_or(record);
    let record = record.strip_suffix(b"\r").unwrap_or(record);
    let decoded = String::from_utf8_lossy(record);
    let command = strip_zsh_history_metadata(&decoded);
    let command = sanitize_detail_label(command);
    if command.is_empty() {
        source.to_owned()
    } else {
        sanitize_detail_label(&format!("{source} · {command}"))
    }
}

/// zsh's extended history starts records with `: <epoch>:<duration>;`. Removing only a prefix
/// that matches this exact shape keeps ordinary commands beginning with a colon untouched.
fn strip_zsh_history_metadata(value: &str) -> &str {
    let Some(metadata) = value.strip_prefix(": ") else {
        return value;
    };
    let Some((metadata, command)) = metadata.split_once(';') else {
        return value;
    };
    let mut fields = metadata.split(':');
    let timestamp = fields.next();
    let duration = fields.next();
    if fields.next().is_none()
        && timestamp.is_some_and(|field| {
            !field.is_empty() && field.bytes().all(|byte| byte.is_ascii_digit())
        })
        && duration.is_some_and(|field| {
            !field.is_empty() && field.bytes().all(|byte| byte.is_ascii_digit())
        })
    {
        command
    } else {
        value
    }
}

fn page_entries(
    entries: Vec<PrivacyDetailEntry>,
    offset: u64,
    limit: u32,
) -> Vec<PrivacyDetailEntry> {
    entries
        .into_iter()
        .skip(usize::try_from(offset).unwrap_or(usize::MAX))
        .take(limit as usize)
        .collect()
}

pub(super) fn platform_detail_entries(
    entries: Vec<mangodisk_platform::PlatformPrivacyDetailEntry>,
) -> (PrivacyDetailsPresentation, Vec<PrivacyDetailEntry>) {
    if entries.is_empty() {
        return (PrivacyDetailsPresentation::AggregateOnly, Vec::new());
    }
    let entries = entries
        .into_iter()
        .map(|entry| PrivacyDetailEntry {
            label: sanitize_detail_label(&entry.label),
            item_count: entry.item_count,
        })
        .collect();
    (PrivacyDetailsPresentation::List, entries)
}

fn path_detail_label(path: &Path) -> String {
    sanitize_detail_label(&path.to_string_lossy())
}

fn sanitize_detail_label(value: &str) -> String {
    value
        .trim()
        .chars()
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .take(MAX_DETAIL_LABEL_CHARS)
        .collect()
}
