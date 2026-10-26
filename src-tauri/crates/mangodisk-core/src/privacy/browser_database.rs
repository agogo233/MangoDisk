mod firefox_passwords;
mod schema;
mod snapshot;

use firefox_passwords::{
    clear_firefox_saved_passwords, count_firefox_saved_passwords, firefox_saved_password_details,
};
use schema::{
    detail_query, require_table_columns, table_exists, table_has_columns, validate_cleanup_schema,
    validate_count_schema,
};
use snapshot::DatabaseSnapshot;

use std::{path::Path, sync::atomic::AtomicU64};

use mangodisk_platform::PlatformPrivacyBrowserKind;
use rusqlite::{Connection, OpenFlags};

use crate::{CoreError, CoreErrorReason, CoreResult};

use super::{PrivacyDataKind, PrivacyDetailEntry, PrivacyTimeRange};

const CHROMIUM_EPOCH_OFFSET_MS: i64 = 11_644_473_600_000;
const SAFARI_EPOCH_OFFSET_SECONDS: i64 = 978_307_200;
const MAX_DETAIL_LABEL_CHARS: usize = 512;

static NEXT_SNAPSHOT: AtomicU64 = AtomicU64::new(1);

pub(super) fn count(
    path: &Path,
    browser: PlatformPrivacyBrowserKind,
    kind: PrivacyDataKind,
    range: PrivacyTimeRange,
    now_ms: u64,
) -> CoreResult<u64> {
    let database_path = safe_database_path(path)?;
    if browser == PlatformPrivacyBrowserKind::Firefox && kind == PrivacyDataKind::SavedPasswords {
        return count_firefox_saved_passwords(&database_path, range);
    }
    match count_at_path(&database_path, browser, kind, range, now_ms, false) {
        Err(error) if error.reason() == Some(CoreErrorReason::ResourceBusy) => {
            // Active browsers can hold an exclusive read lock even though the committed database
            // remains safe to inspect. A short-lived private snapshot lets scanning continue while
            // keeping every destructive operation gated behind browser shutdown and a fresh scan.
            log::debug!("privacy_database_snapshot_fallback reason=resource_busy");
            let snapshot = DatabaseSnapshot::capture(&database_path)?;
            count_at_path(snapshot.path(), browser, kind, range, now_ms, true)
        }
        result => result,
    }
}

pub(super) fn details(
    path: &Path,
    browser: PlatformPrivacyBrowserKind,
    kind: PrivacyDataKind,
    range: PrivacyTimeRange,
    now_ms: u64,
    offset: u64,
    limit: u32,
) -> CoreResult<Vec<PrivacyDetailEntry>> {
    let database_path = safe_database_path(path)?;
    if browser == PlatformPrivacyBrowserKind::Firefox && kind == PrivacyDataKind::SavedPasswords {
        return firefox_saved_password_details(&database_path, offset, limit);
    }
    match details_at_path(
        &database_path,
        browser,
        kind,
        range,
        now_ms,
        offset,
        limit,
        false,
    ) {
        Err(error) if error.reason() == Some(CoreErrorReason::ResourceBusy) => {
            // Detail reads use the same private snapshot fallback as aggregate counting. The copy
            // is deleted before this call returns and no private label is written to diagnostics.
            log::debug!("privacy_database_detail_snapshot_fallback reason=resource_busy");
            let snapshot = DatabaseSnapshot::capture(&database_path)?;
            details_at_path(
                snapshot.path(),
                browser,
                kind,
                range,
                now_ms,
                offset,
                limit,
                true,
            )
        }
        result => result,
    }
}

#[allow(clippy::too_many_arguments)]
fn details_at_path(
    database_path: &Path,
    browser: PlatformPrivacyBrowserKind,
    kind: PrivacyDataKind,
    range: PrivacyTimeRange,
    now_ms: u64,
    offset: u64,
    limit: u32,
    writable_snapshot: bool,
) -> CoreResult<Vec<PrivacyDetailEntry>> {
    let access_flag = if writable_snapshot {
        OpenFlags::SQLITE_OPEN_READ_WRITE
    } else {
        OpenFlags::SQLITE_OPEN_READ_ONLY
    };
    let connection = Connection::open_with_flags(
        database_path,
        access_flag | OpenFlags::SQLITE_OPEN_NO_MUTEX | OpenFlags::SQLITE_OPEN_NOFOLLOW,
    )
    .map_err(database_error)?;
    connection
        .busy_timeout(std::time::Duration::from_millis(250))
        .map_err(database_error)?;
    let cutoff = cutoff_for(range, browser, now_ms);
    let sql = detail_query(browser, kind)?;
    let mut statement = connection.prepare(sql).map_err(database_error)?;
    let limit = i64::from(limit);
    let offset = i64::try_from(offset)
        .map_err(|_| CoreError::invalid_input("privacy detail offset is too large"))?;
    let entries = statement
        .query_map(rusqlite::params![cutoff, limit, offset], |row| {
            let label = row.get::<_, Option<String>>(0)?.unwrap_or_default();
            let item_count = row.get::<_, i64>(1)?;
            Ok(PrivacyDetailEntry {
                label: sanitize_detail_label(&label),
                item_count: u64::try_from(item_count.max(0)).unwrap_or(0),
            })
        })
        .map_err(database_error)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(database_error)?;
    Ok(entries)
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

fn count_at_path(
    database_path: &Path,
    browser: PlatformPrivacyBrowserKind,
    kind: PrivacyDataKind,
    range: PrivacyTimeRange,
    now_ms: u64,
    writable_snapshot: bool,
) -> CoreResult<u64> {
    let access_flag = if writable_snapshot {
        OpenFlags::SQLITE_OPEN_READ_WRITE
    } else {
        OpenFlags::SQLITE_OPEN_READ_ONLY
    };
    let connection = Connection::open_with_flags(
        database_path,
        access_flag | OpenFlags::SQLITE_OPEN_NO_MUTEX | OpenFlags::SQLITE_OPEN_NOFOLLOW,
    )
    .map_err(database_error)?;
    // Process detection is advisory and can race a browser startup. Bound read lock waits so an
    // unrecognized or newly started browser degrades this source instead of freezing the page.
    connection
        .busy_timeout(std::time::Duration::from_millis(250))
        .map_err(database_error)?;
    validate_count_schema(&connection, browser, kind)?;
    let cutoff = cutoff_for(range, browser, now_ms);
    let count: i64 = match (browser, kind, cutoff) {
        (PlatformPrivacyBrowserKind::Chromium, PrivacyDataKind::BrowsingHistory, Some(value)) => {
            connection.query_row(
                "SELECT COUNT(*) FROM visits WHERE visit_time >= ?1",
                [value],
                |row| row.get(0),
            )
        }
        (PlatformPrivacyBrowserKind::Chromium, PrivacyDataKind::BrowsingHistory, None) => {
            connection.query_row("SELECT COUNT(*) FROM visits", [], |row| row.get(0))
        }
        (PlatformPrivacyBrowserKind::Chromium, PrivacyDataKind::SearchHistory, Some(value)) => {
            connection.query_row(
                "SELECT COUNT(*) FROM keyword_search_terms AS terms JOIN urls ON urls.id = terms.url_id WHERE urls.last_visit_time >= ?1",
                [value],
                |row| row.get(0),
            )
        }
        (PlatformPrivacyBrowserKind::Chromium, PrivacyDataKind::SearchHistory, None) => {
            connection.query_row("SELECT COUNT(*) FROM keyword_search_terms", [], |row| row.get(0))
        }
        (PlatformPrivacyBrowserKind::Chromium, PrivacyDataKind::DownloadHistory, Some(value)) => {
            connection.query_row(
                "SELECT COUNT(*) FROM downloads WHERE start_time >= ?1",
                [value],
                |row| row.get(0),
            )
        }
        (PlatformPrivacyBrowserKind::Chromium, PrivacyDataKind::DownloadHistory, None) => {
            connection.query_row("SELECT COUNT(*) FROM downloads", [], |row| row.get(0))
        }
        (PlatformPrivacyBrowserKind::Firefox, PrivacyDataKind::BrowsingHistory, Some(value)) => {
            connection.query_row(
                "SELECT COUNT(*) FROM moz_historyvisits WHERE visit_type <> 7 AND visit_date >= ?1",
                [value],
                |row| row.get(0),
            )
        }
        (PlatformPrivacyBrowserKind::Firefox, PrivacyDataKind::BrowsingHistory, None) => connection
            .query_row(
                "SELECT COUNT(*) FROM moz_historyvisits WHERE visit_type <> 7",
                [],
                |row| row.get(0),
            ),
        (PlatformPrivacyBrowserKind::Firefox, PrivacyDataKind::DownloadHistory, Some(value)) => {
            connection.query_row(
                "SELECT COUNT(*) FROM moz_historyvisits WHERE visit_type = 7 AND visit_date >= ?1",
                [value],
                |row| row.get(0),
            )
        }
        (PlatformPrivacyBrowserKind::Firefox, PrivacyDataKind::DownloadHistory, None) => connection
            .query_row(
                "SELECT COUNT(*) FROM moz_historyvisits WHERE visit_type = 7",
                [],
                |row| row.get(0),
            ),
        (PlatformPrivacyBrowserKind::Safari, PrivacyDataKind::BrowsingHistory, Some(value)) => {
            connection.query_row(
                "SELECT COUNT(*) FROM history_visits WHERE visit_time >= ?1",
                [value],
                |row| row.get(0),
            )
        }
        (PlatformPrivacyBrowserKind::Safari, PrivacyDataKind::BrowsingHistory, None) => {
            connection.query_row("SELECT COUNT(*) FROM history_visits", [], |row| row.get(0))
        }
        (PlatformPrivacyBrowserKind::Chromium, PrivacyDataKind::Cookies, Some(value)) => connection
            .query_row(
                "SELECT COUNT(*) FROM cookies WHERE last_access_utc >= ?1",
                [value],
                |row| row.get(0),
            ),
        (PlatformPrivacyBrowserKind::Chromium, PrivacyDataKind::Cookies, None) => {
            connection.query_row("SELECT COUNT(*) FROM cookies", [], |row| row.get(0))
        }
        (PlatformPrivacyBrowserKind::Firefox, PrivacyDataKind::Cookies, Some(value)) => connection
            .query_row(
                "SELECT COUNT(*) FROM moz_cookies WHERE lastAccessed >= ?1",
                [value],
                |row| row.get(0),
            ),
        (PlatformPrivacyBrowserKind::Firefox, PrivacyDataKind::Cookies, None) => {
            connection.query_row("SELECT COUNT(*) FROM moz_cookies", [], |row| row.get(0))
        }
        (PlatformPrivacyBrowserKind::Firefox, PrivacyDataKind::SitePermissions, None) => {
            connection.query_row("SELECT COUNT(*) FROM moz_perms", [], |row| row.get(0))
        }
        (PlatformPrivacyBrowserKind::Chromium, PrivacyDataKind::SavedPasswords, None) => {
            connection.query_row("SELECT COUNT(*) FROM logins", [], |row| row.get(0))
        }
        (PlatformPrivacyBrowserKind::Chromium, PrivacyDataKind::AutofillData, None) => {
            connection.query_row("SELECT COUNT(*) FROM autofill", [], |row| row.get(0))
        }
        (PlatformPrivacyBrowserKind::Firefox, PrivacyDataKind::AutofillData, None) => {
            connection.query_row("SELECT COUNT(*) FROM moz_formhistory", [], |row| row.get(0))
        }
        (PlatformPrivacyBrowserKind::Chromium, PrivacyDataKind::FrequentlyVisitedSites, None) => {
            connection.query_row("SELECT COUNT(*) FROM top_sites", [], |row| row.get(0))
        }
        (
            PlatformPrivacyBrowserKind::Chromium,
            PrivacyDataKind::AddressBarShortcuts,
            Some(value),
        ) => connection.query_row(
            "SELECT COUNT(*) FROM omni_box_shortcuts WHERE last_access_time >= ?1",
            [value],
            |row| row.get(0),
        ),
        (PlatformPrivacyBrowserKind::Chromium, PrivacyDataKind::AddressBarShortcuts, None) => {
            connection.query_row("SELECT COUNT(*) FROM omni_box_shortcuts", [], |row| {
                row.get(0)
            })
        }
        (PlatformPrivacyBrowserKind::Chromium, PrivacyDataKind::WebsiteIcons, None) => {
            connection.query_row("SELECT COUNT(*) FROM icon_mapping", [], |row| row.get(0))
        }
        _ => {
            return Err(CoreError::operation_failed(
                "privacy database operation is unsupported",
            ))
        }
    }
    .map_err(database_error)?;
    Ok(count.max(0) as u64)
}

pub(super) fn clear(
    path: &Path,
    browser: PlatformPrivacyBrowserKind,
    kind: PrivacyDataKind,
    range: PrivacyTimeRange,
    now_ms: u64,
) -> CoreResult<u64> {
    let database_path = safe_database_path(path)?;
    if browser == PlatformPrivacyBrowserKind::Firefox && kind == PrivacyDataKind::SavedPasswords {
        return clear_firefox_saved_passwords(&database_path, range);
    }
    let before = count(path, browser, kind, range, now_ms)?;
    if before == 0 {
        return Ok(0);
    }
    let mut connection = Connection::open_with_flags(
        database_path,
        OpenFlags::SQLITE_OPEN_READ_WRITE
            | OpenFlags::SQLITE_OPEN_NO_MUTEX
            | OpenFlags::SQLITE_OPEN_NOFOLLOW,
    )
    .map_err(database_error)?;
    connection
        .busy_timeout(std::time::Duration::from_millis(750))
        .map_err(database_error)?;
    validate_cleanup_schema(&connection, browser, kind)?;
    let transaction = connection.transaction().map_err(database_error)?;
    let cutoff = cutoff_for(range, browser, now_ms);
    match (browser, kind, cutoff) {
        (PlatformPrivacyBrowserKind::Chromium, PrivacyDataKind::BrowsingHistory, cutoff) => {
            clear_chromium_history(&transaction, cutoff)?;
        }
        (PlatformPrivacyBrowserKind::Chromium, PrivacyDataKind::SearchHistory, Some(value)) => {
            transaction
                .execute(
                    "DELETE FROM keyword_search_terms WHERE url_id IN (SELECT id FROM urls WHERE last_visit_time >= ?1)",
                    [value],
                )
                .map_err(database_error)?;
        }
        (PlatformPrivacyBrowserKind::Chromium, PrivacyDataKind::SearchHistory, None) => {
            transaction
                .execute("DELETE FROM keyword_search_terms", [])
                .map_err(database_error)?;
        }
        (PlatformPrivacyBrowserKind::Chromium, PrivacyDataKind::DownloadHistory, Some(value)) => {
            transaction.execute("DELETE FROM downloads_url_chains WHERE id IN (SELECT id FROM downloads WHERE start_time >= ?1)", [value]).map_err(database_error)?;
            transaction
                .execute("DELETE FROM downloads WHERE start_time >= ?1", [value])
                .map_err(database_error)?;
        }
        (PlatformPrivacyBrowserKind::Chromium, PrivacyDataKind::DownloadHistory, None) => {
            transaction
                .execute("DELETE FROM downloads_url_chains", [])
                .map_err(database_error)?;
            transaction
                .execute("DELETE FROM downloads", [])
                .map_err(database_error)?;
        }
        (PlatformPrivacyBrowserKind::Firefox, PrivacyDataKind::BrowsingHistory, cutoff) => {
            delete_firefox_visits(&transaction, cutoff, false)?;
        }
        (PlatformPrivacyBrowserKind::Firefox, PrivacyDataKind::DownloadHistory, cutoff) => {
            delete_firefox_visits(&transaction, cutoff, true)?;
        }
        (PlatformPrivacyBrowserKind::Safari, PrivacyDataKind::BrowsingHistory, Some(value)) => {
            transaction
                .execute("DELETE FROM history_visits WHERE visit_time >= ?1", [value])
                .map_err(database_error)?;
            transaction.execute("DELETE FROM history_items WHERE NOT EXISTS (SELECT 1 FROM history_visits WHERE history_visits.history_item = history_items.id)", []).map_err(database_error)?;
        }
        (PlatformPrivacyBrowserKind::Safari, PrivacyDataKind::BrowsingHistory, None) => {
            transaction
                .execute("DELETE FROM history_visits", [])
                .map_err(database_error)?;
            transaction
                .execute("DELETE FROM history_items", [])
                .map_err(database_error)?;
        }
        (PlatformPrivacyBrowserKind::Chromium, PrivacyDataKind::Cookies, Some(value)) => {
            transaction
                .execute("DELETE FROM cookies WHERE last_access_utc >= ?1", [value])
                .map_err(database_error)?;
        }
        (PlatformPrivacyBrowserKind::Chromium, PrivacyDataKind::Cookies, None) => {
            transaction
                .execute("DELETE FROM cookies", [])
                .map_err(database_error)?;
        }
        (PlatformPrivacyBrowserKind::Firefox, PrivacyDataKind::Cookies, Some(value)) => {
            transaction
                .execute("DELETE FROM moz_cookies WHERE lastAccessed >= ?1", [value])
                .map_err(database_error)?;
        }
        (PlatformPrivacyBrowserKind::Firefox, PrivacyDataKind::Cookies, None) => {
            transaction
                .execute("DELETE FROM moz_cookies", [])
                .map_err(database_error)?;
        }
        (PlatformPrivacyBrowserKind::Firefox, PrivacyDataKind::SitePermissions, None) => {
            transaction
                .execute("DELETE FROM moz_perms", [])
                .map_err(database_error)?;
        }
        (PlatformPrivacyBrowserKind::Chromium, PrivacyDataKind::SavedPasswords, None) => {
            // Chromium has accumulated several optional derivative tables across releases. Clear
            // them before the primary login rows so no password notes, health findings, or login
            // statistics survive the user-requested credential removal.
            for table in [
                "insecure_credentials",
                "password_notes",
                "compromised_credentials",
                "stats",
            ] {
                delete_from_optional_table(
                    &transaction,
                    table,
                    &[],
                    &format!("DELETE FROM {table}"),
                )?;
            }
            transaction
                .execute("DELETE FROM logins", [])
                .map_err(database_error)?;
        }
        (PlatformPrivacyBrowserKind::Chromium, PrivacyDataKind::AutofillData, None) => {
            // The scanned `autofill` table is form-entry history. Address profiles, payment cards,
            // and other Web Data tables are deliberately outside this item and remain untouched.
            transaction
                .execute("DELETE FROM autofill", [])
                .map_err(database_error)?;
        }
        (PlatformPrivacyBrowserKind::Firefox, PrivacyDataKind::AutofillData, None) => {
            transaction
                .execute("DELETE FROM moz_formhistory", [])
                .map_err(database_error)?;
        }
        (PlatformPrivacyBrowserKind::Chromium, PrivacyDataKind::FrequentlyVisitedSites, None) => {
            transaction
                .execute("DELETE FROM top_sites", [])
                .map_err(database_error)?;
        }
        (
            PlatformPrivacyBrowserKind::Chromium,
            PrivacyDataKind::AddressBarShortcuts,
            Some(value),
        ) => {
            transaction
                .execute(
                    "DELETE FROM omni_box_shortcuts WHERE last_access_time >= ?1",
                    [value],
                )
                .map_err(database_error)?;
        }
        (PlatformPrivacyBrowserKind::Chromium, PrivacyDataKind::AddressBarShortcuts, None) => {
            transaction
                .execute("DELETE FROM omni_box_shortcuts", [])
                .map_err(database_error)?;
        }
        (PlatformPrivacyBrowserKind::Chromium, PrivacyDataKind::WebsiteIcons, None) => {
            // Icon mappings and bitmap payloads are derived from visited pages. Clear dependent
            // rows first, preserve the schema and meta table, and let Chromium repopulate icons.
            transaction
                .execute("DELETE FROM icon_mapping", [])
                .map_err(database_error)?;
            transaction
                .execute("DELETE FROM favicon_bitmaps", [])
                .map_err(database_error)?;
            transaction
                .execute("DELETE FROM favicons", [])
                .map_err(database_error)?;
        }
        _ => {
            return Err(CoreError::operation_failed(
                "privacy database operation is unsupported",
            ))
        }
    }
    transaction.commit().map_err(database_error)?;
    let remaining = count(path, browser, kind, range, now_ms)?;
    if remaining != 0 {
        return Err(CoreError::operation_failed(
            "privacy database verification failed",
        ));
    }
    Ok(before)
}

fn clear_chromium_history(
    transaction: &rusqlite::Transaction<'_>,
    cutoff: Option<i64>,
) -> CoreResult<()> {
    // Chromium stores derived privacy data in several version-dependent tables. Capture the
    // selected visit IDs before deleting the primary rows, then remove only matching derivatives.
    // Optional tables are skipped when absent but fail closed when present with an unknown schema.
    transaction
        .execute_batch(
            "CREATE TEMP TABLE mangodisk_privacy_visits(
                 id INTEGER PRIMARY KEY
             ) WITHOUT ROWID;",
        )
        .map_err(database_error)?;
    match cutoff {
        Some(value) => transaction.execute(
            "INSERT INTO mangodisk_privacy_visits SELECT id FROM visits WHERE visit_time >= ?1",
            [value],
        ),
        None => transaction.execute(
            "INSERT INTO mangodisk_privacy_visits SELECT id FROM visits",
            [],
        ),
    }
    .map_err(database_error)?;

    delete_from_optional_table(
        transaction,
        "visit_source",
        &["id"],
        "DELETE FROM visit_source WHERE id IN (SELECT id FROM mangodisk_privacy_visits)",
    )?;
    for table in ["content_annotations", "context_annotations"] {
        delete_from_optional_table(
            transaction,
            table,
            &["visit_id"],
            &format!(
                "DELETE FROM {table} WHERE visit_id IN (SELECT id FROM mangodisk_privacy_visits)"
            ),
        )?;
    }
    delete_from_optional_table(
        transaction,
        "clusters_and_visits",
        &["cluster_id", "visit_id"],
        "DELETE FROM clusters_and_visits WHERE visit_id IN (SELECT id FROM mangodisk_privacy_visits)",
    )?;
    delete_from_optional_table(
        transaction,
        "cluster_visit_duplicates",
        &["visit_id", "duplicate_visit_id"],
        "DELETE FROM cluster_visit_duplicates WHERE visit_id IN (SELECT id FROM mangodisk_privacy_visits) OR duplicate_visit_id IN (SELECT id FROM mangodisk_privacy_visits)",
    )?;

    // Remaining visits can contain navigation links to a removed visit. Reset those aggregate
    // identifiers so Chromium does not retain a dangling trace or reopen a deleted relationship.
    if table_has_columns(transaction, "visits", &["from_visit"])? {
        transaction
            .execute(
                "UPDATE visits SET from_visit = 0 WHERE from_visit IN (SELECT id FROM mangodisk_privacy_visits)",
                [],
            )
            .map_err(database_error)?;
    }
    if table_has_columns(transaction, "visits", &["opener_visit"])? {
        transaction
            .execute(
                "UPDATE visits SET opener_visit = 0 WHERE opener_visit IN (SELECT id FROM mangodisk_privacy_visits)",
                [],
            )
            .map_err(database_error)?;
    }
    transaction
        .execute(
            "DELETE FROM visits WHERE id IN (SELECT id FROM mangodisk_privacy_visits)",
            [],
        )
        .map_err(database_error)?;

    if table_exists(transaction, "visited_links")? {
        require_table_columns(transaction, "visited_links", &["id"])?;
        require_table_columns(transaction, "visits", &["visited_link_id"])?;
        transaction
            .execute(
                "DELETE FROM visited_links WHERE NOT EXISTS (SELECT 1 FROM visits WHERE visits.visited_link_id = visited_links.id)",
                [],
            )
            .map_err(database_error)?;
    }
    if table_exists(transaction, "segments")? {
        require_table_columns(transaction, "segments", &["id", "url_id"])?;
        delete_from_optional_table(
            transaction,
            "segment_usage",
            &["segment_id"],
            "DELETE FROM segment_usage WHERE segment_id IN (SELECT segments.id FROM segments WHERE NOT EXISTS (SELECT 1 FROM visits WHERE visits.url = segments.url_id))",
        )?;
        transaction
            .execute(
                "DELETE FROM segments WHERE NOT EXISTS (SELECT 1 FROM visits WHERE visits.url = segments.url_id)",
                [],
            )
            .map_err(database_error)?;
    }
    delete_from_optional_table(
        transaction,
        "keyword_search_terms",
        &["url_id"],
        "DELETE FROM keyword_search_terms WHERE NOT EXISTS (SELECT 1 FROM visits WHERE visits.url = keyword_search_terms.url_id)",
    )?;
    transaction
        .execute(
            "DELETE FROM urls WHERE NOT EXISTS (SELECT 1 FROM visits WHERE visits.url = urls.id)",
            [],
        )
        .map_err(database_error)?;
    if table_exists(transaction, "clusters")? && table_exists(transaction, "clusters_and_visits")? {
        require_table_columns(transaction, "clusters", &["cluster_id"])?;
        require_table_columns(transaction, "clusters_and_visits", &["cluster_id"])?;
        transaction
            .execute(
                "DELETE FROM clusters WHERE NOT EXISTS (SELECT 1 FROM clusters_and_visits WHERE clusters_and_visits.cluster_id = clusters.cluster_id)",
                [],
            )
            .map_err(database_error)?;
    }
    Ok(())
}

fn delete_from_optional_table(
    connection: &Connection,
    table: &str,
    required_columns: &[&str],
    statement: &str,
) -> CoreResult<()> {
    if !table_exists(connection, table)? {
        return Ok(());
    }
    require_table_columns(connection, table, required_columns)?;
    connection.execute(statement, []).map_err(database_error)?;
    Ok(())
}

fn delete_firefox_visits(
    transaction: &rusqlite::Transaction<'_>,
    cutoff: Option<i64>,
    downloads: bool,
) -> CoreResult<()> {
    let comparison = if downloads { "=" } else { "<>" };
    let sql = match cutoff {
        Some(_) => format!(
            "DELETE FROM moz_historyvisits WHERE visit_type {comparison} 7 AND visit_date >= ?1"
        ),
        None => format!("DELETE FROM moz_historyvisits WHERE visit_type {comparison} 7"),
    };
    match cutoff {
        Some(value) => transaction.execute(&sql, [value]),
        None => transaction.execute(&sql, []),
    }
    .map_err(database_error)?;
    // Bookmark-owned places must survive even after their final history visit is removed.
    transaction.execute(
        "DELETE FROM moz_places WHERE foreign_count = 0 AND NOT EXISTS (SELECT 1 FROM moz_historyvisits WHERE moz_historyvisits.place_id = moz_places.id) AND NOT EXISTS (SELECT 1 FROM moz_bookmarks WHERE moz_bookmarks.fk = moz_places.id)",
        [],
    ).map_err(database_error)?;
    Ok(())
}

fn cutoff_for(
    range: PrivacyTimeRange,
    browser: PlatformPrivacyBrowserKind,
    now_ms: u64,
) -> Option<i64> {
    let cutoff_ms = match range {
        PrivacyTimeRange::LastHour => now_ms.saturating_sub(60 * 60 * 1_000),
        PrivacyTimeRange::Today => {
            local_midnight_ms(now_ms).unwrap_or_else(|| now_ms - (now_ms % 86_400_000))
        }
        PrivacyTimeRange::LastSevenDays => now_ms.saturating_sub(7 * 24 * 60 * 60 * 1_000),
        PrivacyTimeRange::AllTime => return None,
    } as i64;
    Some(match browser {
        PlatformPrivacyBrowserKind::Chromium => (cutoff_ms + CHROMIUM_EPOCH_OFFSET_MS) * 1_000,
        PlatformPrivacyBrowserKind::Firefox => cutoff_ms * 1_000,
        PlatformPrivacyBrowserKind::Safari => cutoff_ms / 1_000 - SAFARI_EPOCH_OFFSET_SECONDS,
    })
}

fn local_midnight_ms(now_ms: u64) -> Option<u64> {
    use chrono::{Local, TimeZone};

    let instant = Local.timestamp_millis_opt(now_ms as i64).single()?;
    let midnight = instant.date_naive().and_hms_opt(0, 0, 0)?;
    let local_midnight = Local.from_local_datetime(&midnight).earliest()?;
    u64::try_from(local_midnight.timestamp_millis()).ok()
}

fn safe_database_path(path: &Path) -> CoreResult<std::path::PathBuf> {
    let metadata = std::fs::symlink_metadata(path).map_err(|error| {
        CoreError::operation_failed(format!(
            "privacy database identity is unavailable kind={:?}",
            error.kind()
        ))
    })?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(CoreError::operation_failed(
            "privacy database identity is unsafe",
        ));
    }
    std::fs::canonicalize(path).map_err(|error| {
        CoreError::operation_failed(format!(
            "privacy database canonicalization failed kind={:?}",
            error.kind()
        ))
    })
}

fn database_error(error: rusqlite::Error) -> CoreError {
    let reason = match error.sqlite_error_code() {
        Some(rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked) => {
            Some(CoreErrorReason::ResourceBusy)
        }
        _ => None,
    };
    let diagnostic = match error {
        rusqlite::Error::SqliteFailure(code, _) => format!(
            "privacy database operation failed code={:?} extended_code={}",
            code.code, code.extended_code
        ),
        _ => "privacy database operation failed".into(),
    };
    let error = CoreError::operation_failed(diagnostic);
    match reason {
        Some(reason) => error.with_reason(reason),
        None => error,
    }
}

#[cfg(test)]
#[path = "browser_database_tests.rs"]
mod tests;
