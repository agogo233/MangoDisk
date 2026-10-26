//! Per-user Explorer integration shared by installed and portable desktop builds.
//! No HKLM registration, elevated child, or developer-mode bypass is used here.
use serde::Serialize;
use std::{
    fs,
    io::{self, Write},
    os::windows::fs::MetadataExt,
    path::{Path, PathBuf},
    sync::{Mutex, MutexGuard},
    time::UNIX_EPOCH,
};
use windows::{
    core::{HSTRING, PWSTR},
    Foundation::Uri,
    Management::Deployment::{AddPackageOptions, PackageManager},
    Win32::{
        System::WinRT::{RoInitialize, RoUninitialize, RO_INIT_MULTITHREADED},
        UI::Shell::UrlCreateFromPathW,
    },
};
use winreg::{
    enums::{HKEY_CURRENT_USER, KEY_READ, KEY_WOW64_64KEY},
    RegKey,
};

const KEY: &str = r"Software\MangoDisk\ExplorerIntegration";
const CLSID: &str = "{93E49F90-CEBD-4BA0-93BA-733A3C732214}";
const PACKAGE: &str = "MangoDisk.ExplorerIntegration";
static MUTATION: Mutex<()> = Mutex::new(());

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    pub enabled: bool,
}

pub struct Payload<'a> {
    pub dll: &'a [u8],
    pub identity: &'a [u8],
    pub logo: &'a [u8],
}

pub fn status() -> io::Result<Status> {
    let _guard = lock_registration()?;
    read_status()
}

fn lock_registration() -> io::Result<MutexGuard<'static, ()>> {
    MUTATION
        .lock()
        .map_err(|_| io::Error::other("Explorer registration lock poisoned"))
}

fn read_status() -> io::Result<Status> {
    match RegKey::predef(HKEY_CURRENT_USER).open_subkey(KEY) {
        Ok(key) => Ok(Status {
            enabled: key.get_value::<u32, _>("Enabled").unwrap_or(0) == 1,
        }),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Status { enabled: false }),
        Err(error) => Err(error),
    }
}

/// NSIS invokes the existing desktop executable before removing its files.
/// An installation must not remove a portable copy's newer registration.
pub fn run_removal_mode(arguments: impl IntoIterator<Item = std::ffi::OsString>) -> Option<i32> {
    let arguments: Vec<_> = arguments.into_iter().collect();
    if arguments
        .get(1)
        .is_none_or(|argument| argument != "--remove-explorer-integration")
    {
        return None;
    }
    if arguments.len() != 2 {
        return Some(2);
    }
    Some(match remove_current_registration() {
        Ok(()) => 0,
        Err(error) => {
            eprintln!("explorer_integration_remove_failed error={error}");
            1
        }
    })
}

fn remove_current_registration() -> io::Result<()> {
    let current_user = RegKey::predef(HKEY_CURRENT_USER);
    let key = match current_user
        .open_subkey_with_flags(KEY, winreg::enums::KEY_READ | winreg::enums::KEY_WRITE)
    {
        Ok(key) => key,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    let owner: String = key.get_value("Executable")?;
    if fs::canonicalize(owner)? != fs::canonicalize(std::env::current_exe()?)? {
        return Ok(());
    }
    key.set_value("Enabled", &0u32)?;
    unregister_menu()?;
    current_user.delete_subkey_all(KEY)?;
    Ok(())
}

fn unregister_menu() -> io::Result<()> {
    if windows_build()? >= 22000 {
        let _runtime = Runtime::new()?;
        remove_identity(&PackageManager::new().map_err(native)?)?;
    }
    unregister_classic()
}

fn unregister_classic() -> io::Result<()> {
    let current_user = RegKey::predef(HKEY_CURRENT_USER);
    for context in [r"Directory", r"Directory\Background"] {
        let path = format!(r"Software\Classes\{context}\shell\MangoDisk.Analyze");
        remove_owned_verb(&current_user, &path)?;
    }
    let class = format!(r"Software\Classes\CLSID\{CLSID}");
    ignore_missing(current_user.delete_subkey_all(class))
}

fn ignore_missing(result: io::Result<()>) -> io::Result<()> {
    match result {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        other => other,
    }
}

fn remove_owned_verb(current_user: &RegKey, path: &str) -> io::Result<()> {
    let handler = current_user
        .open_subkey(path)
        .and_then(|key| key.get_value::<String, _>("ExplorerCommandHandler"));
    match handler {
        Ok(handler) if handler == CLSID => ignore_missing(current_user.delete_subkey_all(path)),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

pub fn set_enabled(enabled: bool, title: &str, payload: Payload<'_>) -> io::Result<Status> {
    let _guard = lock_registration()?;
    set_enabled_locked(enabled, title, payload)
}

pub fn refresh_if_enabled(title: &str, payload: Payload<'_>) -> io::Result<()> {
    let _guard = lock_registration()?;
    // Recheck the preference inside the same operation that refreshes it, so a
    // delayed startup refresh cannot undo the user's completed disable action.
    if read_status()?.enabled {
        set_enabled_locked(true, title, payload)?;
    }
    Ok(())
}

fn set_enabled_locked(enabled: bool, title: &str, payload: Payload<'_>) -> io::Result<Status> {
    let (key, _) = RegKey::predef(HKEY_CURRENT_USER).create_subkey(KEY)?;
    if !enabled {
        key.set_value("Enabled", &0u32)?;
        unregister_menu()?;
        return read_status();
    }
    if title.is_empty() || title.len() > 512 || title.chars().any(char::is_control) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid Explorer command title",
        ));
    }
    let executable = std::env::current_exe()?;
    let modern = windows_build()? >= 22000;
    let directory = stage(&executable, &payload, modern)?;
    let dll = directory.join("MangoDiskExplorer.dll");
    let old_directory = key
        .get_value::<String, _>("Directory")
        .ok()
        .map(PathBuf::from);
    if old_directory.as_deref() != Some(&directory) || !read_status()?.enabled {
        // Never claim enabled while a first registration or update is incomplete.
        key.set_value("Enabled", &0u32)?;
        if modern {
            register_identity(&directory)?;
        }
    }
    if modern {
        // Packaged verbs also appear in the classic menu. Registering both
        // mechanisms on Windows 11 creates duplicate entries there.
        unregister_classic()?;
    } else {
        register_classic(&dll, title)?;
    }
    key.set_value("SchemaVersion", &1u32)?;
    key.set_value("Executable", &executable.as_os_str())?;
    key.set_value("MenuTitle", &title)?;
    key.set_value("Directory", &directory.as_os_str())?;
    key.set_value("Enabled", &1u32)?;
    cleanup_old_versions(&directory);
    log::info!(
        "explorer_integration_updated enabled=true executable={} directory={} modern={modern}",
        crate::diagnostics::text(&executable.display()),
        crate::diagnostics::text(&directory.display())
    );
    read_status()
}

fn stage(executable: &Path, payload: &Payload<'_>, modern: bool) -> io::Result<PathBuf> {
    let root =
        dirs::data_local_dir().ok_or_else(|| io::Error::other("LocalAppData is unavailable"))?;
    let metadata = fs::metadata(executable)?;
    let mut hash = blake3::Hasher::new();
    hash.update(payload.dll);
    hash.update(payload.identity);
    hash.update(&metadata.len().to_le_bytes());
    hash.update(
        &metadata
            .modified()?
            .duration_since(UNIX_EPOCH)
            .map_err(io::Error::other)?
            .as_nanos()
            .to_le_bytes(),
    );
    let directory = root
        .join("app.mangodisk.desktop/explorer-integration")
        .join(hash.finalize().to_hex().as_str());
    // Validate each application-owned ancestor, not just the final DLL path.
    for ancestor in directory
        .ancestors()
        .take(3)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
    {
        fs::create_dir_all(ancestor)?;
        reject_reparse(ancestor)?;
    }
    write_payload(&directory.join("MangoDiskExplorer.dll"), payload.dll)?;
    if modern {
        write_payload(&directory.join("identity.msix"), payload.identity)?;
        write_payload(&directory.join("logo.png"), payload.logo)?;
        // Sparse package applications require a stable executable beneath their
        // external root. Reuse this exact main executable without another helper
        // implementation. A hard link adds no image copy on the usual same-volume
        // layout; portable executables on another volume use an ordinary copy.
        let anchor = directory.join("MangoDisk.exe");
        match fs::symlink_metadata(&anchor) {
            Ok(_) => reject_reparse(&anchor)?,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                match fs::hard_link(executable, &anchor) {
                    Ok(()) => (),
                    Err(_) => {
                        let mut temporary = tempfile::NamedTempFile::new_in(&directory)?;
                        io::copy(&mut fs::File::open(executable)?, temporary.as_file_mut())?;
                        temporary.as_file().sync_all()?;
                        temporary
                            .persist_noclobber(&anchor)
                            .map_err(|error| error.error)?;
                    }
                }
            }
            Err(error) => return Err(error),
        }
        reject_reparse(&anchor)?;
    }
    Ok(directory)
}

fn reject_reparse(path: &Path) -> io::Result<()> {
    if fs::symlink_metadata(path)?.file_attributes() & 0x400 != 0 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!(
                "Explorer component path is a reparse point: {}",
                path.display()
            ),
        ));
    }
    Ok(())
}

fn write_payload(path: &Path, bytes: &[u8]) -> io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(_) => {
            reject_reparse(path)?;
            if fs::read(path)? == bytes {
                return Ok(());
            }
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "Explorer component differs from embedded bytes: {}",
                    path.display()
                ),
            ));
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => (),
        Err(error) => return Err(error),
    }
    let mut temporary = tempfile::NamedTempFile::new_in(
        path.parent()
            .ok_or_else(|| io::Error::other("component parent unavailable"))?,
    )?;
    temporary.write_all(bytes)?;
    temporary.as_file().sync_all()?;
    temporary
        .persist_noclobber(path)
        .map_err(|error| error.error)?;
    Ok(())
}

fn cleanup_old_versions(current: &Path) {
    let Some(root) = current.parent() else {
        return;
    };
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if path == current
            || name.len() != 64
            || !name.bytes().all(|value| value.is_ascii_hexdigit())
            || reject_reparse(&path).is_err()
        {
            continue;
        }
        // Only these exact application-owned files are disposable. Mapped DLLs
        // and running executable links can survive until a later launch.
        for name in [
            "MangoDiskExplorer.dll",
            "MangoDisk.exe",
            "identity.msix",
            "logo.png",
        ] {
            let _ = fs::remove_file(path.join(name));
        }
        let _ = fs::remove_dir(path);
    }
}

fn register_classic(dll: &Path, title: &str) -> io::Result<()> {
    let current_user = RegKey::predef(HKEY_CURRENT_USER);
    let (server, _) =
        current_user.create_subkey(format!(r"Software\Classes\CLSID\{CLSID}\InprocServer32"))?;
    server.set_value("", &dll.as_os_str())?;
    server.set_value("ThreadingModel", &"Apartment")?;
    let (verb, _) =
        current_user.create_subkey(r"Software\Classes\Directory\shell\MangoDisk.Analyze")?;
    verb.set_value("ExplorerCommandHandler", &CLSID)?;
    verb.set_value("MUIVerb", &title)?;
    Ok(())
}

struct Runtime;
impl Runtime {
    fn new() -> io::Result<Self> {
        unsafe { RoInitialize(RO_INIT_MULTITHREADED) }.map_err(native)?;
        Ok(Self)
    }
}
impl Drop for Runtime {
    fn drop(&mut self) {
        unsafe { RoUninitialize() };
    }
}

fn register_identity(directory: &Path) -> io::Result<()> {
    let _runtime = Runtime::new()?;
    let manager = PackageManager::new().map_err(native)?;
    // Same-version development rebuilds and portable relocation need an explicit
    // replacement. Scope enumeration and removal to this user's exact package.
    remove_identity(&manager)?;
    let options = AddPackageOptions::new().map_err(native)?;
    options
        .SetExternalLocationUri(&file_uri(directory)?)
        .map_err(native)?;
    let operation = manager
        .AddPackageByUriAsync(&file_uri(&directory.join("identity.msix"))?, &options)
        .map_err(native)?;
    check_deployment(operation.join().map_err(native)?)
}

fn remove_identity(manager: &PackageManager) -> io::Result<()> {
    let packages = manager
        .FindPackagesByUserSecurityId(&HSTRING::new())
        .map_err(native)?;
    for package in packages {
        let id = package.Id().map_err(native)?;
        if id.Name().map_err(native)? == PACKAGE {
            let result = manager
                .RemovePackageAsync(&id.FullName().map_err(native)?)
                .map_err(native)?
                .join()
                .map_err(native)?;
            check_deployment(result)?;
        }
    }
    Ok(())
}

fn check_deployment(result: windows::Management::Deployment::DeploymentResult) -> io::Result<()> {
    let code = result.ExtendedErrorCode().map_err(native)?;
    if code.is_err() {
        return Err(io::Error::other(format!(
            "Explorer identity registration failed HRESULT={code} error={}",
            result.ErrorText().map_err(native)?
        )));
    }
    Ok(())
}

fn file_uri(path: &Path) -> io::Result<Uri> {
    // WinRT file URIs use Windows' native escaping conventions. Feeding a
    // UTF-8 percent-encoded URL to CreateUri corrupts non-ASCII path segments.
    let path = HSTRING::from(path.as_os_str());
    let mut buffer = vec![0u16; path.len() * 3 + 16];
    let mut length = u32::try_from(buffer.len())
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
    unsafe { UrlCreateFromPathW(&path, PWSTR(buffer.as_mut_ptr()), &mut length, 0) }
        .map_err(native)?;
    Uri::CreateUri(&HSTRING::from_wide(&buffer[..length as usize])).map_err(native)
}

fn native(error: windows::core::Error) -> io::Error {
    io::Error::other(error)
}

fn windows_build() -> io::Result<u32> {
    RegKey::predef(winreg::enums::HKEY_LOCAL_MACHINE)
        .open_subkey_with_flags(
            r"SOFTWARE\Microsoft\Windows NT\CurrentVersion",
            KEY_READ | KEY_WOW64_64KEY,
        )?
        .get_value::<String, _>("CurrentBuildNumber")?
        .parse()
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn status_waits_for_an_in_flight_registration() {
        let mutation = MUTATION.lock().unwrap();
        let (started, ready) = std::sync::mpsc::channel();
        let (sender, receiver) = std::sync::mpsc::channel();
        let reader = std::thread::spawn(move || {
            started.send(()).unwrap();
            sender.send(status().is_ok()).unwrap();
        });
        ready.recv().unwrap();
        let premature = receiver.recv_timeout(std::time::Duration::from_millis(200));
        drop(mutation);
        reader.join().unwrap();
        assert!(
            matches!(premature, Err(std::sync::mpsc::RecvTimeoutError::Timeout)),
            "settings must not observe a registration's temporary disabled state"
        );
    }

    #[test]
    fn staged_payload_never_overwrites_existing_different_bytes() {
        let root = tempfile::tempdir().unwrap();
        let file = root.path().join("component.dll");
        write_payload(&file, b"first").unwrap();
        write_payload(&file, b"first").unwrap();
        assert!(write_payload(&file, b"different").is_err());
        assert_eq!(fs::read(file).unwrap(), b"first");
    }
    #[test]
    fn removing_owned_verb_is_idempotent_and_preserves_other_handlers() {
        let current_user = RegKey::predef(HKEY_CURRENT_USER);
        let path = format!(
            r"Software\MangoDisk\ExplorerIntegrationTests\{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        remove_owned_verb(&current_user, &path).unwrap();
        let (key, _) = current_user.create_subkey(&path).unwrap();
        key.set_value("ExplorerCommandHandler", &"another-handler")
            .unwrap();
        remove_owned_verb(&current_user, &path).unwrap();
        assert!(current_user.open_subkey(&path).is_ok());
        key.set_value("ExplorerCommandHandler", &CLSID).unwrap();
        drop(key);
        remove_owned_verb(&current_user, &path).unwrap();
        remove_owned_verb(&current_user, &path).unwrap();
        assert!(current_user.open_subkey(&path).is_err());
    }
    #[test]
    fn component_file_uri_preserves_spaces_unicode_and_hash() {
        let uri = file_uri(Path::new(
            "C:\\folder space\\\u{4e2d}\u{6587} #1\\identity.msix",
        ))
        .unwrap();
        let path = uri.Path().unwrap().to_string();
        let fragment = uri.Fragment().unwrap().to_string();
        assert!(
            fragment.is_empty(),
            "file URI fragment must be empty: {fragment:?}"
        );
        assert!(
            path.contains("#1/identity.msix"),
            "file URI path lost its hash: {path:?}"
        );
        assert!(
            path.contains("\u{4e2d}\u{6587}"),
            "file URI path lost Unicode: {path:?}"
        );
    }
}
