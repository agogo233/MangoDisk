//! Deterministic numeric text shared by native resident displays.

pub(super) fn compact_rate(bytes: f64, base: f64) -> (String, &'static str) {
    if !bytes.is_finite() || bytes < 0.0 {
        return ("—".into(), "");
    }
    let units = ["B", "K", "M", "G", "T"];
    let mut number = bytes;
    let mut unit = 0;
    while number >= base - 0.5 && unit < units.len() - 1 {
        number /= base;
        unit += 1;
    }
    // Three digits fit at the normal numeric font size. Never shrink until an
    // arbitrary value fits; the last unit explicitly caps exceptionally large rates.
    (format!("{:.0}", number.min(999.0)), units[unit])
}

pub(super) fn byte_text(bytes: f64, base: f64) -> String {
    byte_text_with_precision(bytes, base, 1)
}

pub(super) fn byte_text_with_precision(bytes: f64, base: f64, precision: usize) -> String {
    let mut value = bytes;
    let units = ["B", "KB", "MB", "GB", "TB"];
    let mut index = 0;
    while value >= base && index < units.len() - 1 {
        value /= base;
        index += 1;
    }
    format!("{value:.precision$} {}", units[index])
}

/// Constant-width fields keep native status-bar layout stable across unit changes.
pub(super) fn rate_title(bytes: f64, base: f64) -> String {
    let units = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes;
    let mut index = 0;
    while (value * 10.0).round() / 10.0 >= base && index < units.len() - 1 {
        value /= base;
        index += 1;
    }
    format!("{:>5.1} {:>2}/s", value.min(999.9), units[index])
}

pub(super) fn displayed_power(value: f64) -> f64 {
    // Sub-watt readings display as a range, so integer color rules use its
    // lower bound rather than treating a nonzero reading as idle.
    if value < 1.0 {
        0.0
    } else {
        value.round()
    }
}

pub(super) fn power_digits(value: f64) -> String {
    if value > 0.0 && value < 1.0 {
        "<1".into()
    } else {
        format!("{:.0}", value.round())
    }
}
