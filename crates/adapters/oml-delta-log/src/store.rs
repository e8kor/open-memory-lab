//! The log of a table on a local disk.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;

use crate::{LogError, LogStore, unreadable};

/// `<root>/_delta_log/`, from which it opens a file by bare name and nothing
/// else: a name with a separator, a `..` or a leading dot-dot is refused, so a
/// data file, a sibling table or a path outside the log is out of reach.
#[derive(Debug, Clone)]
pub struct DirStore {
    log: PathBuf,
}

impl DirStore {
    /// The store of the table at `root`.
    #[must_use]
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            log: root.into().join("_delta_log"),
        }
    }

    /// The log directory, refused when it is a link: a table never reads
    /// another directory's log through one.
    fn dir(&self) -> Result<&PathBuf, LogError> {
        let meta = std::fs::symlink_metadata(&self.log)
            .map_err(|e| unreadable(format!("{}: {e}", self.log.display())))?;
        if meta.file_type().is_symlink() {
            return Err(unreadable(format!(
                "{} is a symlink: a log is never read through one",
                self.log.display()
            )));
        }
        Ok(&self.log)
    }

    /// A regular file by bare name, never a link.
    fn file(&self, name: &str) -> Result<PathBuf, LogError> {
        let path = self.path(name)?;
        let ft = std::fs::symlink_metadata(&path)
            .map_err(|e| io(name, &e))?
            .file_type();
        if ft.is_symlink() {
            return Err(unreadable(format!(
                "{name} is a symlink: a log file is never read through one"
            )));
        }
        if !ft.is_file() {
            return Err(unreadable(format!("{name} is not a regular file")));
        }
        Ok(path)
    }

    fn path(&self, name: &str) -> Result<PathBuf, LogError> {
        if name.is_empty() || name == "." || name == ".." || name.contains(['/', '\\', '\0']) {
            return Err(unreadable(format!(
                "{name:?} is not a file directly under _delta_log"
            )));
        }
        Ok(self.dir()?.join(name))
    }
}

fn io(name: &str, e: &std::io::Error) -> LogError {
    unreadable(format!("{name}: {e}"))
}

impl LogStore for DirStore {
    fn list(&self) -> Result<Vec<String>, LogError> {
        let mut out = Vec::new();
        let entries = std::fs::read_dir(self.dir()?)
            .map_err(|e| unreadable(format!("{}: {e}", self.log.display())))?;
        for entry in entries {
            let entry = entry.map_err(|e| io("_delta_log", &e))?;
            if entry.file_type().is_ok_and(|t| t.is_file())
                && let Some(name) = entry.file_name().to_str()
            {
                out.push(name.to_owned());
            }
        }
        out.sort();
        Ok(out)
    }

    fn len(&self, name: &str) -> Result<u64, LogError> {
        std::fs::metadata(self.file(name)?)
            .map(|m| m.len())
            .map_err(|e| io(name, &e))
    }

    fn get(&self, name: &str) -> Result<Vec<u8>, LogError> {
        std::fs::read(self.file(name)?).map_err(|e| io(name, &e))
    }

    fn get_range(&self, name: &str, start: u64, len: u64) -> Result<Vec<u8>, LogError> {
        let mut f = File::open(self.file(name)?).map_err(|e| io(name, &e))?;
        f.seek(SeekFrom::Start(start)).map_err(|e| io(name, &e))?;
        let mut out = Vec::new();
        f.take(len)
            .read_to_end(&mut out)
            .map_err(|e| io(name, &e))?;
        Ok(out)
    }
}
