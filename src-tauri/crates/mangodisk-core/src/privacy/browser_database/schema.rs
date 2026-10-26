use super::{database_error, PrivacyDataKind};
use crate::{CoreError, CoreResult};
use mangodisk_platform::PlatformPrivacyBrowserKind;
use rusqlite::Connection;

pub(super) fn detail_query(
    browser: PlatformPrivacyBrowserKind,
    kind: PrivacyDataKind,
) -> CoreResult<&'static str> {
    let sql = match (browser, kind) {
        (PlatformPrivacyBrowserKind::Chromium, PrivacyDataKind::BrowsingHistory) => {
            "SELECT urls.url, COUNT(*) FROM visits JOIN urls ON urls.id = visits.url WHERE (?1 IS NULL OR visits.visit_time >= ?1) GROUP BY urls.url ORDER BY MAX(visits.visit_time) DESC, urls.url LIMIT ?2 OFFSET ?3"
        }
        (PlatformPrivacyBrowserKind::Firefox, PrivacyDataKind::BrowsingHistory) => {
            "SELECT moz_places.url, COUNT(*) FROM moz_historyvisits JOIN moz_places ON moz_places.id = moz_historyvisits.place_id WHERE moz_historyvisits.visit_type <> 7 AND (?1 IS NULL OR moz_historyvisits.visit_date >= ?1) GROUP BY moz_places.url ORDER BY MAX(moz_historyvisits.visit_date) DESC, moz_places.url LIMIT ?2 OFFSET ?3"
        }
        (PlatformPrivacyBrowserKind::Safari, PrivacyDataKind::BrowsingHistory) => {
            "SELECT history_items.url, COUNT(*) FROM history_visits JOIN history_items ON history_items.id = history_visits.history_item WHERE (?1 IS NULL OR history_visits.visit_time >= ?1) GROUP BY history_items.url ORDER BY MAX(history_visits.visit_time) DESC, history_items.url LIMIT ?2 OFFSET ?3"
        }
        (PlatformPrivacyBrowserKind::Chromium, PrivacyDataKind::DownloadHistory) => {
            "SELECT target_path, COUNT(*) FROM downloads WHERE (?1 IS NULL OR start_time >= ?1) GROUP BY target_path ORDER BY MAX(start_time) DESC, target_path LIMIT ?2 OFFSET ?3"
        }
        (PlatformPrivacyBrowserKind::Firefox, PrivacyDataKind::DownloadHistory) => {
            "SELECT moz_places.url, COUNT(*) FROM moz_historyvisits JOIN moz_places ON moz_places.id = moz_historyvisits.place_id WHERE moz_historyvisits.visit_type = 7 AND (?1 IS NULL OR moz_historyvisits.visit_date >= ?1) GROUP BY moz_places.url ORDER BY MAX(moz_historyvisits.visit_date) DESC, moz_places.url LIMIT ?2 OFFSET ?3"
        }
        (PlatformPrivacyBrowserKind::Chromium, PrivacyDataKind::SearchHistory) => {
            "SELECT keyword_search_terms.term, COUNT(*) FROM keyword_search_terms JOIN urls ON urls.id = keyword_search_terms.url_id WHERE (?1 IS NULL OR urls.last_visit_time >= ?1) GROUP BY keyword_search_terms.term ORDER BY MAX(urls.last_visit_time) DESC, keyword_search_terms.term LIMIT ?2 OFFSET ?3"
        }
        (PlatformPrivacyBrowserKind::Chromium, PrivacyDataKind::Cookies) => {
            "SELECT host_key, COUNT(*) FROM cookies WHERE (?1 IS NULL OR last_access_utc >= ?1) GROUP BY host_key ORDER BY COUNT(*) DESC, host_key LIMIT ?2 OFFSET ?3"
        }
        (PlatformPrivacyBrowserKind::Firefox, PrivacyDataKind::Cookies) => {
            "SELECT host, COUNT(*) FROM moz_cookies WHERE (?1 IS NULL OR lastAccessed >= ?1) GROUP BY host ORDER BY COUNT(*) DESC, host LIMIT ?2 OFFSET ?3"
        }
        (PlatformPrivacyBrowserKind::Firefox, PrivacyDataKind::SitePermissions) => {
            "SELECT CASE WHEN type IS NULL OR type = '' THEN origin ELSE origin || ' · ' || type END, COUNT(*) FROM moz_perms GROUP BY origin, type ORDER BY origin, type LIMIT ?2 OFFSET ?3"
        }
        (PlatformPrivacyBrowserKind::Chromium, PrivacyDataKind::SavedPasswords) => {
            "SELECT origin_url, COUNT(*) FROM logins GROUP BY origin_url ORDER BY origin_url LIMIT ?2 OFFSET ?3"
        }
        (PlatformPrivacyBrowserKind::Chromium, PrivacyDataKind::AutofillData) => {
            "SELECT name, COUNT(*) FROM autofill GROUP BY name ORDER BY name LIMIT ?2 OFFSET ?3"
        }
        (PlatformPrivacyBrowserKind::Firefox, PrivacyDataKind::AutofillData) => {
            "SELECT fieldname, COUNT(*) FROM moz_formhistory GROUP BY fieldname ORDER BY fieldname LIMIT ?2 OFFSET ?3"
        }
        (PlatformPrivacyBrowserKind::Chromium, PrivacyDataKind::FrequentlyVisitedSites) => {
            "SELECT url, COUNT(*) FROM top_sites GROUP BY url ORDER BY MIN(url_rank), url LIMIT ?2 OFFSET ?3"
        }
        (PlatformPrivacyBrowserKind::Chromium, PrivacyDataKind::AddressBarShortcuts) => {
            "SELECT text, COUNT(*) FROM omni_box_shortcuts WHERE (?1 IS NULL OR last_access_time >= ?1) GROUP BY text ORDER BY MAX(last_access_time) DESC, text LIMIT ?2 OFFSET ?3"
        }
        (PlatformPrivacyBrowserKind::Chromium, PrivacyDataKind::WebsiteIcons) => {
            "SELECT page_url, COUNT(*) FROM icon_mapping GROUP BY page_url ORDER BY page_url LIMIT ?2 OFFSET ?3"
        }
        _ => {
            return Err(CoreError::operation_failed(
                "privacy database details are unsupported",
            ));
        }
    };
    Ok(sql)
}

pub(super) fn table_exists(connection: &Connection, table: &str) -> CoreResult<bool> {
    connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1)",
            [table],
            |row| row.get::<_, bool>(0),
        )
        .map_err(database_error)
}

pub(super) fn table_has_columns(
    connection: &Connection,
    table: &str,
    required_columns: &[&str],
) -> CoreResult<bool> {
    let mut statement = connection
        .prepare(&format!("PRAGMA table_info({table})"))
        .map_err(database_error)?;
    let columns = statement
        .query_map([], |row| row.get::<_, String>(1))
        .map_err(database_error)?
        .collect::<Result<std::collections::BTreeSet<_>, _>>()
        .map_err(database_error)?;
    Ok(required_columns
        .iter()
        .all(|column| columns.contains(*column)))
}

pub(super) fn require_table_columns(
    connection: &Connection,
    table: &str,
    required_columns: &[&str],
) -> CoreResult<()> {
    if table_has_columns(connection, table, required_columns)? {
        Ok(())
    } else {
        Err(CoreError::operation_failed(
            "privacy database schema is unsupported",
        ))
    }
}

pub(super) fn validate_count_schema(
    connection: &Connection,
    browser: PlatformPrivacyBrowserKind,
    kind: PrivacyDataKind,
) -> CoreResult<()> {
    let required = match (browser, kind) {
        (PlatformPrivacyBrowserKind::Chromium, PrivacyDataKind::BrowsingHistory) => {
            &[("visits", &["visit_time"] as &[_])][..]
        }
        (PlatformPrivacyBrowserKind::Chromium, PrivacyDataKind::SearchHistory) => &[
            ("keyword_search_terms", &["url_id"] as &[_]),
            ("urls", &["id", "last_visit_time"]),
        ][..],
        (PlatformPrivacyBrowserKind::Chromium, PrivacyDataKind::DownloadHistory) => {
            &[("downloads", &["start_time"] as &[_])][..]
        }
        (
            PlatformPrivacyBrowserKind::Firefox,
            PrivacyDataKind::BrowsingHistory | PrivacyDataKind::DownloadHistory,
        ) => &[("moz_historyvisits", &["visit_type", "visit_date"] as &[_])][..],
        (PlatformPrivacyBrowserKind::Safari, PrivacyDataKind::BrowsingHistory) => {
            &[("history_visits", &["visit_time"] as &[_])][..]
        }
        (PlatformPrivacyBrowserKind::Chromium, PrivacyDataKind::Cookies) => {
            &[("cookies", &["last_access_utc"] as &[_])][..]
        }
        (PlatformPrivacyBrowserKind::Firefox, PrivacyDataKind::Cookies) => {
            &[("moz_cookies", &["lastAccessed"] as &[_])][..]
        }
        (PlatformPrivacyBrowserKind::Firefox, PrivacyDataKind::SitePermissions) => {
            &[("moz_perms", &[] as &[_])][..]
        }
        (PlatformPrivacyBrowserKind::Chromium, PrivacyDataKind::SavedPasswords) => {
            &[("logins", &[] as &[_])][..]
        }
        (PlatformPrivacyBrowserKind::Chromium, PrivacyDataKind::AutofillData) => {
            &[("autofill", &[] as &[_])][..]
        }
        (PlatformPrivacyBrowserKind::Firefox, PrivacyDataKind::AutofillData) => {
            &[("moz_formhistory", &[] as &[_])][..]
        }
        (PlatformPrivacyBrowserKind::Chromium, PrivacyDataKind::FrequentlyVisitedSites) => {
            &[("top_sites", &["url"] as &[_])][..]
        }
        (PlatformPrivacyBrowserKind::Chromium, PrivacyDataKind::AddressBarShortcuts) => {
            &[("omni_box_shortcuts", &["last_access_time"] as &[_])][..]
        }
        (PlatformPrivacyBrowserKind::Chromium, PrivacyDataKind::WebsiteIcons) => {
            &[("icon_mapping", &["icon_id"] as &[_])][..]
        }
        _ => {
            return Err(CoreError::operation_failed(
                "privacy database schema is unsupported",
            ))
        }
    };
    validate_required_tables(connection, required)
}

pub(super) fn validate_cleanup_schema(
    connection: &Connection,
    browser: PlatformPrivacyBrowserKind,
    kind: PrivacyDataKind,
) -> CoreResult<()> {
    let required = match (browser, kind) {
        (PlatformPrivacyBrowserKind::Chromium, PrivacyDataKind::BrowsingHistory) => &[
            ("visits", &["url", "visit_time"] as &[_]),
            ("urls", &["id"]),
        ][..],
        (PlatformPrivacyBrowserKind::Chromium, PrivacyDataKind::SearchHistory) => &[
            ("keyword_search_terms", &["url_id"] as &[_]),
            ("urls", &["id", "last_visit_time"]),
        ][..],
        (PlatformPrivacyBrowserKind::Chromium, PrivacyDataKind::DownloadHistory) => &[
            ("downloads", &["id", "start_time"] as &[_]),
            ("downloads_url_chains", &["id"]),
        ][..],
        (
            PlatformPrivacyBrowserKind::Firefox,
            PrivacyDataKind::BrowsingHistory | PrivacyDataKind::DownloadHistory,
        ) => &[
            (
                "moz_historyvisits",
                &["place_id", "visit_type", "visit_date"] as &[_],
            ),
            ("moz_places", &["id", "foreign_count"]),
            ("moz_bookmarks", &["fk"]),
        ][..],
        (PlatformPrivacyBrowserKind::Safari, PrivacyDataKind::BrowsingHistory) => &[
            ("history_visits", &["history_item", "visit_time"] as &[_]),
            ("history_items", &["id"]),
        ][..],
        (PlatformPrivacyBrowserKind::Chromium, PrivacyDataKind::Cookies) => {
            &[("cookies", &["last_access_utc"] as &[_])][..]
        }
        (PlatformPrivacyBrowserKind::Firefox, PrivacyDataKind::Cookies) => {
            &[("moz_cookies", &["lastAccessed"] as &[_])][..]
        }
        (PlatformPrivacyBrowserKind::Firefox, PrivacyDataKind::SitePermissions) => {
            &[("moz_perms", &[] as &[_])][..]
        }
        (PlatformPrivacyBrowserKind::Chromium, PrivacyDataKind::SavedPasswords) => {
            &[("logins", &[] as &[_])][..]
        }
        (PlatformPrivacyBrowserKind::Chromium, PrivacyDataKind::AutofillData) => {
            &[("autofill", &[] as &[_])][..]
        }
        (PlatformPrivacyBrowserKind::Firefox, PrivacyDataKind::AutofillData) => {
            &[("moz_formhistory", &[] as &[_])][..]
        }
        (PlatformPrivacyBrowserKind::Chromium, PrivacyDataKind::FrequentlyVisitedSites) => {
            &[("top_sites", &["url"] as &[_])][..]
        }
        (PlatformPrivacyBrowserKind::Chromium, PrivacyDataKind::AddressBarShortcuts) => {
            &[("omni_box_shortcuts", &["last_access_time"] as &[_])][..]
        }
        (PlatformPrivacyBrowserKind::Chromium, PrivacyDataKind::WebsiteIcons) => &[
            ("icon_mapping", &["icon_id"] as &[_]),
            ("favicon_bitmaps", &["icon_id"]),
            ("favicons", &["id"]),
        ][..],
        _ => {
            return Err(CoreError::operation_failed(
                "privacy database schema is unsupported",
            ))
        }
    };
    validate_required_tables(connection, required)
}

fn validate_required_tables(
    connection: &Connection,
    required: &[(&str, &[&str])],
) -> CoreResult<()> {
    for (table, required_columns) in required {
        let exists: i64 = connection
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
                [table],
                |row| row.get(0),
            )
            .map_err(database_error)?;
        if exists != 1 {
            return Err(CoreError::operation_failed(
                "privacy database schema is unsupported",
            ));
        }
        let mut statement = connection
            .prepare(&format!("PRAGMA table_info({table})"))
            .map_err(database_error)?;
        let columns = statement
            .query_map([], |row| row.get::<_, String>(1))
            .map_err(database_error)?
            .collect::<Result<std::collections::BTreeSet<_>, _>>()
            .map_err(database_error)?;
        if required_columns
            .iter()
            .any(|column| !columns.contains(*column))
        {
            return Err(CoreError::operation_failed(
                "privacy database schema is unsupported",
            ));
        }
    }
    Ok(())
}
