//! Fixed installation files are staged before stopping a running service.
use std::{
    fs::{self, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
};

pub(crate) struct Replacement {
    target: PathBuf,
    pending: PathBuf,
    previous: PathBuf,
    backed_up: bool,
    replaced: bool,
}
impl Replacement {
    pub(crate) fn prepare(directory: &Path, name: &str, bytes: &[u8]) -> io::Result<Self> {
        let target = directory.join(name);
        let pending = directory.join(format!("{name}.next"));
        let previous = directory.join(format!("{name}.previous"));
        // Recover an interrupted rename before replacing any committed bytes.
        if previous.exists() && !target.exists() {
            fs::rename(&previous, &target)?;
        }
        if pending.exists() {
            fs::remove_file(&pending)?;
        }
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&pending)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        Ok(Self {
            target,
            pending,
            previous,
            backed_up: false,
            replaced: false,
        })
    }
    pub(crate) fn apply(&mut self) -> io::Result<()> {
        if self.previous.exists() {
            fs::remove_file(&self.previous)?;
        }
        if self.target.exists() {
            fs::rename(&self.target, &self.previous)?;
            self.backed_up = true;
        }
        fs::rename(&self.pending, &self.target)?;
        self.replaced = true;
        Ok(())
    }
    pub(crate) fn rollback(&mut self) -> io::Result<()> {
        if self.replaced {
            fs::remove_file(&self.target)?;
            self.replaced = false;
        }
        if self.backed_up {
            fs::rename(&self.previous, &self.target)?;
            self.backed_up = false;
        }
        Ok(())
    }
    pub(crate) fn commit(&mut self) -> io::Result<()> {
        self.backed_up = false;
        self.replaced = false;
        if self.previous.exists() {
            fs::remove_file(&self.previous)?;
        }
        Ok(())
    }
}
impl Drop for Replacement {
    fn drop(&mut self) {
        if self.pending.exists() {
            let _ = fs::remove_file(&self.pending);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn failed_replacement_restores_exact_previous_bytes() {
        let directory = tempfile::tempdir().unwrap();
        let target = directory.path().join("service.exe");
        fs::write(&target, b"previous").unwrap();
        let mut replacement =
            Replacement::prepare(directory.path(), "service.exe", b"next").unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"previous");
        replacement.apply().unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"next");
        replacement.rollback().unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"previous");
        replacement.rollback().unwrap();
    }
    #[test]
    fn first_install_rollback_removes_only_its_new_target() {
        let directory = tempfile::tempdir().unwrap();
        fs::write(directory.path().join("unrelated"), b"untouched").unwrap();
        let mut replacement =
            Replacement::prepare(directory.path(), "service.exe", b"new").unwrap();
        replacement.apply().unwrap();
        replacement.rollback().unwrap();
        assert!(!directory.path().join("service.exe").exists());
        assert_eq!(
            fs::read(directory.path().join("unrelated")).unwrap(),
            b"untouched"
        );
    }
    #[test]
    fn interrupted_rename_is_recovered_before_preparing_a_new_version() {
        let directory = tempfile::tempdir().unwrap();
        fs::write(directory.path().join("service.exe.previous"), b"last_good").unwrap();
        let mut replacement =
            Replacement::prepare(directory.path(), "service.exe", b"next").unwrap();
        assert_eq!(
            fs::read(directory.path().join("service.exe")).unwrap(),
            b"last_good"
        );
        replacement.apply().unwrap();
        replacement.commit().unwrap();
        assert!(!directory.path().join("service.exe.previous").exists());
    }
}
