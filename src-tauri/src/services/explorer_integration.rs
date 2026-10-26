//! Assemble embedded desktop resources without coupling platform code to Tauri.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub enabled: bool,
}

#[cfg(windows)]
fn embedded_payload() -> mangodisk_platform::explorer_integration::Payload<'static> {
    mangodisk_platform::explorer_integration::Payload {
        dll: include_bytes!(concat!(env!("OUT_DIR"), "/MangoDiskExplorer.dll")),
        identity: include_bytes!(concat!(env!("OUT_DIR"), "/explorer_identity.msix")),
        logo: include_bytes!(concat!(env!("OUT_DIR"), "/logo.png")),
    }
}

pub fn status() -> std::io::Result<Status> {
    #[cfg(windows)]
    {
        mangodisk_platform::explorer_integration::status().map(|value| Status {
            enabled: value.enabled,
        })
    }
    #[cfg(not(windows))]
    {
        Ok(Status { enabled: false })
    }
}

pub fn set_enabled(enabled: bool, title: &str) -> std::io::Result<Status> {
    #[cfg(windows)]
    {
        mangodisk_platform::explorer_integration::set_enabled(enabled, title, embedded_payload())
            .map(|value| Status {
                enabled: value.enabled,
            })
    }
    #[cfg(not(windows))]
    {
        let _ = (enabled, title);
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "Explorer integration requires Windows",
        ))
    }
}

#[cfg(windows)]
pub fn refresh(app: &tauri::AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let result = (|| {
            if status()?.enabled {
                let labels = super::native_labels::NativeLabels::load(&app);
                mangodisk_platform::explorer_integration::refresh_if_enabled(
                    labels.text("/settings/explorerIntegration/command"),
                    embedded_payload(),
                )?;
            }
            Ok::<_, std::io::Error>(())
        })();
        if let Err(error) = result {
            log::warn!(
                "explorer_integration_refresh_failed error={}",
                mangodisk_platform::diagnostics::text(&error)
            );
        }
    });
}
