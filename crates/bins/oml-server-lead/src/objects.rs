//! The object store port's local-disk adapter (ADR-0004 d3, as amended
//! 2026-11-17; ADR-0090 d16): `storage.url` names a directory, an object is
//! a file under it named by the object's name, written to a temporary file
//! and renamed into place, owner-only. Another scheme is refused at start
//! until an object store adapter exists.

use std::path::{Path, PathBuf};

use oml_app::ports::{ObjectFuture, ObjectStore};

/// `storage.url`'s default: a directory beside the keyring's default.
pub const DEFAULT_STORAGE_URL: &str = "oml-objects";

/// The directory `storage.url` names: an absolute or relative path, or a
/// `file://` URL; any other scheme is refused by name.
///
/// # Errors
///
/// A scheme other than `file`, or an empty value, naming the setting.
pub fn storage_dir(url: &str) -> Result<PathBuf, String> {
    let url = url.trim();
    let path = match url.split_once("://") {
        None => url,
        Some(("file", rest)) => rest,
        Some((scheme, _)) => {
            return Err(format!(
                "storage.url's scheme {scheme:?} is not served: this build stores objects in a local directory only, a path or a file:// URL (ADR-0004 d3)"
            ));
        }
    };
    if path.is_empty() {
        return Err(
            "storage.url is empty: a local directory is a path or a file:// URL (ADR-0004 d3)"
                .to_owned(),
        );
    }
    Ok(PathBuf::from(path))
}

/// Objects as files under one directory, created owner-only at open.
#[derive(Debug, Clone)]
pub struct LocalObjects {
    root: PathBuf,
}

impl LocalObjects {
    /// The store over `root`, creating it owner-only where it is missing.
    ///
    /// # Errors
    ///
    /// The directory cannot be created, or is not a directory.
    pub fn open(root: &Path) -> Result<Self, String> {
        create_private_dir(root).map_err(|e| format!("storage.url {}: {e}", root.display()))?;
        // A root that existed before is tightened to owner-only too
        // (ADR-0004 d3, readings of 2026-11-18).
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(root, std::fs::Permissions::from_mode(0o700))
                .map_err(|e| format!("storage.url {}: {e}", root.display()))?;
        }
        Ok(Self {
            root: root.to_path_buf(),
        })
    }

    /// The file an object name maps to; a name with an empty, `.` or `..`
    /// segment, or an absolute one, maps to none.
    fn path_of(&self, name: &str) -> Result<PathBuf, String> {
        let mut path = self.root.clone();
        for segment in name.split('/') {
            if segment.is_empty() || segment == "." || segment == ".." || segment.contains('\\') {
                return Err(format!("object name {name:?} is not a relative name"));
            }
            path.push(segment);
        }
        Ok(path)
    }
}

fn create_private_dir(dir: &Path) -> std::io::Result<()> {
    let mut builder = std::fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(dir)
}

fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let parent = path.parent().unwrap_or(Path::new("."));
    create_private_dir(parent)?;
    // A fresh random name opened exclusively: no file or link left at a
    // fixed name is ever written through (ADR-0004 d3, readings of
    // 2026-11-18).
    let mut nonce = [0_u8; 16];
    getrandom::getrandom(&mut nonce).map_err(std::io::Error::other)?;
    let tmp = parent.join(format!(
        ".{}.partial",
        nonce.iter().map(|b| format!("{b:02x}")).collect::<String>()
    ));
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&tmp)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);
    if let Err(e) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    // The rename is durable once the directory that holds it is synced.
    std::fs::File::open(parent)?.sync_all()
}

fn dir_bytes(path: &Path) -> std::io::Result<u64> {
    let meta = match std::fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(0),
        Err(e) => return Err(e),
    };
    if meta.is_file() {
        return Ok(meta.len());
    }
    if !meta.is_dir() {
        return Ok(0);
    }
    let mut total = 0;
    for entry in std::fs::read_dir(path)? {
        total += dir_bytes(&entry?.path())?;
    }
    Ok(total)
}

async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> std::io::Result<T> + Send + 'static,
) -> Result<T, String> {
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|e| format!("object store task: {e}"))?
        .map_err(|e| e.to_string())
}

impl ObjectStore for LocalObjects {
    fn put<'a>(&'a self, name: &'a str, bytes: Vec<u8>) -> ObjectFuture<'a, ()> {
        Box::pin(async move {
            let path = self.path_of(name)?;
            blocking(move || write_private(&path, &bytes)).await
        })
    }

    fn get<'a>(&'a self, name: &'a str) -> ObjectFuture<'a, Option<Vec<u8>>> {
        Box::pin(async move {
            let path = self.path_of(name)?;
            blocking(move || match std::fs::read(&path) {
                Ok(bytes) => Ok(Some(bytes)),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
                Err(e) => Err(e),
            })
            .await
        })
    }

    fn delete_prefix<'a>(&'a self, prefix: &'a str) -> ObjectFuture<'a, ()> {
        Box::pin(async move {
            // The upload prefixes end in `/`: the prefix is a directory.
            let path = self.path_of(prefix.trim_end_matches('/'))?;
            blocking(move || match std::fs::remove_dir_all(&path) {
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
                other => other,
            })
            .await
        })
    }

    fn stored_bytes<'a>(&'a self, prefix: &'a str) -> ObjectFuture<'a, u64> {
        Box::pin(async move {
            let path = self.path_of(prefix.trim_end_matches('/'))?;
            blocking(move || dir_bytes(&path)).await
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn storage_url_is_a_path_or_a_file_url_and_any_other_scheme_is_refused() {
        assert_eq!(storage_dir("oml-objects"), Ok(PathBuf::from("oml-objects")));
        assert_eq!(
            storage_dir("/srv/oml/objects"),
            Ok(PathBuf::from("/srv/oml/objects"))
        );
        assert_eq!(
            storage_dir("file:///srv/oml/objects"),
            Ok(PathBuf::from("/srv/oml/objects"))
        );
        let refused = storage_dir("s3://bucket/objects").expect_err("refused");
        assert!(refused.contains("\"s3\""), "{refused}");
        assert!(storage_dir("  ").is_err());
    }

    #[tokio::test]
    async fn an_object_round_trips_owner_only_and_a_prefix_is_removed_whole() {
        let dir = std::env::temp_dir().join(format!("oml-objects-{}", std::process::id()));
        let store = LocalObjects::open(&dir).expect("open");
        store
            .put("uploads/t/u/000001", b"one".to_vec())
            .await
            .expect("put");
        store
            .put("uploads/t/u/000002", b"two!".to_vec())
            .await
            .expect("put");
        assert_eq!(
            store.get("uploads/t/u/000001").await.expect("get"),
            Some(b"one".to_vec())
        );
        assert_eq!(store.get("uploads/t/u/000009").await.expect("get"), None);
        assert_eq!(store.stored_bytes("uploads/").await.expect("bytes"), 7);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(dir.join("uploads/t/u/000001"))
                .expect("meta")
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        assert!(store.put("../escape", b"x".to_vec()).await.is_err());
        assert!(store.get("uploads//x").await.is_err());
        store.delete_prefix("uploads/t/u/").await.expect("delete");
        store
            .delete_prefix("uploads/t/u/")
            .await
            .expect("deleting nothing is no error");
        assert_eq!(store.stored_bytes("uploads/").await.expect("bytes"), 0);
        std::fs::remove_dir_all(&dir).expect("clean up");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_put_never_writes_through_a_link_at_a_temporary_name_and_the_root_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("oml-objects-link-{}", std::process::id()));
        let outside = std::env::temp_dir().join(format!("oml-objects-out-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("uploads/t/u")).expect("dirs");
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).expect("chmod");
        std::fs::write(&outside, b"untouched").expect("outside");
        std::os::unix::fs::symlink(&outside, dir.join("uploads/t/u/000001.partial"))
            .expect("a link at the old fixed name");
        let store = LocalObjects::open(&dir).expect("open");
        assert_eq!(
            std::fs::metadata(&dir).expect("meta").permissions().mode() & 0o777,
            0o700,
            "an existing root is tightened"
        );
        store
            .put("uploads/t/u/000001", b"one".to_vec())
            .await
            .expect("put");
        assert_eq!(std::fs::read(&outside).expect("read"), b"untouched");
        assert_eq!(
            store.get("uploads/t/u/000001").await.expect("get"),
            Some(b"one".to_vec())
        );
        std::fs::remove_dir_all(&dir).expect("clean up");
        std::fs::remove_file(&outside).expect("clean up");
    }
}
