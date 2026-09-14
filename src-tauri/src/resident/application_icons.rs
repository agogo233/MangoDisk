//! Warm presentation assets through the same native cache used by file-icon IPC.
use mangodisk_core::system_resources::models::ProcessMemorySummary;
use mangodisk_platform::{
    NativeFileIconItemKind, NativeFileIconMode, NativeFileIconRequest, NativeFileIconService,
};
use tauri::Manager;

pub fn warm(app: &tauri::AppHandle, summary: &ProcessMemorySummary) {
    let started = std::time::Instant::now();
    let requests = requests(summary);
    let requested = requests.len();
    // Match get_file_icons: one persistent cache serves every WebView, including
    // a monitor WebView that has not been created during background startup.
    let cache = app
        .path()
        .app_cache_dir()
        .ok()
        .map(|path| path.join("cache").join("file-icons"));
    let result = NativeFileIconService::load(requests, cache);
    log::info!(
        "resident_icons_warmed requested={requested} assets={} cache_hits={} system_lookups={} elapsed_ms={}",
        result.assets.len(), result.cache_hits, result.system_lookups, started.elapsed().as_millis()
    );
}

fn requests(summary: &ProcessMemorySummary) -> Vec<NativeFileIconRequest> {
    summary
        .applications
        .iter()
        .filter_map(|application| {
            Some(NativeFileIconRequest {
                path: application.icon_path.clone()?,
                kind: if application.is_bundle {
                    NativeFileIconItemKind::Directory
                } else {
                    NativeFileIconItemKind::File
                },
                mode: if application.is_bundle {
                    NativeFileIconMode::Path
                } else {
                    NativeFileIconMode::Automatic
                },
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use mangodisk_core::system_resources::models::ApplicationMemory;

    #[test]
    fn prewarming_matches_row_icon_identities_and_skips_missing_paths() {
        let summary = ProcessMemorySummary {
            applications: [
                (Some("/Editor.app"), true),
                (Some("/editor.exe"), false),
                (None, false),
            ]
            .into_iter()
            .map(|(path, is_bundle)| ApplicationMemory {
                id: "application".into(),
                name: "Application".into(),
                resident_bytes: 10,
                process_count: 1,
                icon_path: path.map(String::from),
                is_bundle,
                can_quit: false,
            })
            .collect(),
            readable_process_count: 3,
            omitted_process_count: 0,
        };
        let requests = requests(&summary);
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0].kind, NativeFileIconItemKind::Directory);
        assert_eq!(requests[0].mode, NativeFileIconMode::Path);
        assert_eq!(requests[1].kind, NativeFileIconItemKind::File);
        assert_eq!(requests[1].mode, NativeFileIconMode::Automatic);
    }
}
