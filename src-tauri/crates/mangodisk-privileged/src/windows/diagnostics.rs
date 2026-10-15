//! Lifecycle events remain inspectable when SCM provides no console stream.
use super::security;
use windows_sys::Win32::System::EventLog::{
    DeregisterEventSource, RegisterEventSourceW, ReportEventW, EVENTLOG_ERROR_TYPE,
    EVENTLOG_INFORMATION_TYPE,
};

pub(crate) fn error(message: impl AsRef<str>) {
    emit(message.as_ref(), EVENTLOG_ERROR_TYPE);
}

pub(crate) fn info(message: impl AsRef<str>) {
    emit(message.as_ref(), EVENTLOG_INFORMATION_TYPE);
}

fn bounded(message: &str) -> String {
    message
        .chars()
        .flat_map(|character| {
            if character.is_control() {
                character.escape_default().collect::<Vec<_>>()
            } else {
                vec![character]
            }
        })
        .take(2048)
        .collect()
}

fn emit(message: &str, level: u16) {
    let bounded = bounded(message);
    eprintln!("{bounded}");
    let source = unsafe {
        RegisterEventSourceW(
            std::ptr::null(),
            security::wide("MangoDiskPrivileged").as_ptr(),
        )
    };
    if source.is_null() {
        return;
    }
    let message = security::wide(bounded);
    let strings = [message.as_ptr()];
    unsafe {
        ReportEventW(
            source,
            level,
            0,
            1,
            std::ptr::null_mut(),
            1,
            0,
            strings.as_ptr(),
            std::ptr::null(),
        );
        DeregisterEventSource(source);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagnostics_escape_controls_and_bound_the_expanded_message() {
        assert_eq!(bounded("path=a\nerror=5\t"), "path=a\\nerror=5\\t");
        assert_eq!(bounded(&"\n".repeat(2048)).len(), 2048);
        assert_eq!(bounded(&"\u{754c}".repeat(4096)).chars().count(), 2048);
    }
}
