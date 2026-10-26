use std::sync::Once;

pub(crate) fn install_panic_logging() {
    static INSTALL: Once = Once::new();
    INSTALL.call_once(|| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            let thread = std::thread::current();
            let location = info
                .location()
                .map(|location| {
                    format!(
                        "{}:{}:{}",
                        location.file(),
                        location.line(),
                        location.column()
                    )
                })
                .unwrap_or_else(|| "unknown".into());
            // Panic payloads may contain arbitrary application data. Source location and thread
            // identify the failing code without copying that data into exported diagnostics.
            log::error!(
                "application_panicked process_id={} thread={} location={} payload_logged=false",
                std::process::id(),
                mangodisk_platform::diagnostics::text(thread.name().unwrap_or("unnamed")),
                mangodisk_platform::diagnostics::text(&location)
            );
            previous(info);
        }));
    });
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    struct Evidence(Mutex<Vec<String>>);
    impl log::Log for Evidence {
        fn enabled(&self, _: &log::Metadata<'_>) -> bool {
            true
        }
        fn log(&self, record: &log::Record<'_>) {
            self.0.lock().unwrap().push(record.args().to_string());
        }
        fn flush(&self) {}
    }

    #[test]
    #[ignore = "installs process-wide panic and log hooks; run in isolation"]
    fn panic_logging_keeps_location_without_arbitrary_payload() {
        static LOGS: Evidence = Evidence(Mutex::new(Vec::new()));
        log::set_logger(&LOGS).unwrap();
        log::set_max_level(log::LevelFilter::Info);
        super::install_panic_logging();
        assert!(std::panic::catch_unwind(|| panic!("synthetic-private-panic-payload")).is_err());
        let logs = LOGS.0.lock().unwrap().clone();
        let event = logs
            .iter()
            .find(|line| line.starts_with("application_panicked "))
            .unwrap();
        assert!(event.contains("process_id="));
        assert!(event.contains("runtime_diagnostics.rs:"));
        assert!(event.contains("payload_logged=false"));
        assert!(!event.contains("synthetic-private-panic-payload"));
    }
}
