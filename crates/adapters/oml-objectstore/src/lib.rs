//! `oml-objectstore` — the one object-store port over the Rust `object_store`
//! crate (proposed ADR-0035 d10): every stored object, and once the lake is
//! built every lake table, lives in the one store `storage.url` picks.
//!
//! - **One URL.** [`ObjectStorage::open`] classifies `storage.url` with the
//!   crate's own [`ObjectStoreScheme::parse`] and builds the store with
//!   [`object_store::parse_url_opts`] from it and `storage.options`, the
//!   crate's endpoint and credential keys; [`ObjectStorage::store`] hands the
//!   same store to any other adapter, as `oml-lake` will take it.
//! - **The default.** Unset or empty, `storage.url` is the `file://` URL of
//!   [`DEFAULT_DATA_DIRECTORY`] in the working directory, so a basic install
//!   runs no storage service.
//! - **The file store syncs.** A `file://` URL builds a
//!   [`DurableLocalFileSystem`], which returns from a write only once its file
//!   and directory entries are synced (d10, "Two obligations on every write").
//! - **Put-if-absent.** Every put but a part's is `PutMode::Create`, so a
//!   stored object's name is written once ([`ObjectStorage::put`]); a part is a
//!   plain put a retry replaces ([`ObjectStorage::put_part`]). No write asks
//!   the store for a compare-and-set: those stay in Postgres.
//! - **Refused schemes.** `memory` forgets every object at restart and
//!   `http`/`https` is a store the suite does not cover, so opening either is
//!   refused, naming the setting.
//!
//! The crate is wired into no binary yet (d12 step S1).

mod local;

use std::collections::BTreeMap;
use std::path::Path as FsPath;
use std::sync::Arc;

use bytes::Bytes;
use futures_util::TryStreamExt;
pub use local::DurableLocalFileSystem;
pub use object_store;
use object_store::path::Path;
use object_store::{ObjectStore, ObjectStoreExt, ObjectStoreScheme, PutMode, PutPayload};
use url::Url;

/// The setting that picks the backend (proposed ADR-0035 d10).
pub const STORAGE_URL: &str = "storage.url";

/// The setting carrying the object_store crate's endpoint and credential
/// keys, such as `aws_endpoint` (proposed ADR-0035 d10).
pub const STORAGE_OPTIONS: &str = "storage.options";

/// The directory, in the working directory, an unset `storage.url` stores
/// under (proposed ADR-0035 d10, "The default").
pub const DEFAULT_DATA_DIRECTORY: &str = "oml-data";

/// `storage.url` and `storage.options` as configuration reads them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StorageSettings {
    /// The store's URL; `None` or empty is the default.
    pub url: Option<String>,
    /// The object_store crate's configuration keys for the URL's scheme.
    pub options: BTreeMap<String, String>,
}

/// Why opening the store, or one of its operations, failed.
#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    /// `storage.url` does not parse, or names no scheme the crate knows.
    #[error("storage.url `{url}` is not a store URL: {reason}")]
    InvalidUrl { url: String, reason: String },
    /// `storage.url` names a scheme startup refuses (proposed ADR-0035 d10).
    #[error("storage.url names the `{scheme}` scheme, which is refused: {reason}")]
    RefusedScheme {
        scheme: String,
        reason: &'static str,
    },
    /// The `file://` directory could not be created or opened.
    #[error("storage.url's directory `{path}` cannot be used: {source}")]
    Directory {
        path: String,
        source: std::io::Error,
    },
    /// A put-if-absent found an object already stored at the name.
    #[error("an object is already stored at `{name}`")]
    AlreadyExists { name: String },
    /// No object is stored at the name.
    #[error("no object is stored at `{name}`")]
    NotFound { name: String },
    /// Any other failure the store reported.
    #[error("object store: {0}")]
    Store(#[source] object_store::Error),
}

impl StorageError {
    fn from_store(name: &Path, err: object_store::Error) -> Self {
        match err {
            object_store::Error::AlreadyExists { .. } => Self::AlreadyExists {
                name: name.to_string(),
            },
            object_store::Error::NotFound { .. } => Self::NotFound {
                name: name.to_string(),
            },
            other => Self::Store(other),
        }
    }
}

/// The one store `storage.url` picks, with names relative to its root.
#[derive(Debug, Clone)]
pub struct ObjectStorage {
    store: Arc<dyn ObjectStore>,
    root: Path,
    url: Url,
}

impl ObjectStorage {
    /// Opens the store `settings` name, the default under `working_dir` when
    /// `storage.url` is unset or empty. A `file://` directory is created when
    /// missing; `memory` and `http`/`https` are refused (proposed ADR-0035 d10).
    pub fn open(settings: &StorageSettings, working_dir: &FsPath) -> Result<Self, StorageError> {
        let url = match settings.url.as_deref().filter(|u| !u.is_empty()) {
            Some(raw) => Url::parse(raw).map_err(|e| StorageError::InvalidUrl {
                url: raw.to_owned(),
                reason: e.to_string(),
            })?,
            None => {
                let dir = create_directory(&working_dir.join(DEFAULT_DATA_DIRECTORY))?;
                Url::from_directory_path(&dir).map_err(|()| StorageError::InvalidUrl {
                    url: dir.display().to_string(),
                    reason: "the data directory is not an absolute path".to_owned(),
                })?
            }
        };
        let invalid = |reason: String| StorageError::InvalidUrl {
            url: url.to_string(),
            reason,
        };
        let (scheme, _) = ObjectStoreScheme::parse(&url).map_err(|e| invalid(e.to_string()))?;
        let (store, root): (Arc<dyn ObjectStore>, Path) = match scheme {
            ObjectStoreScheme::Memory => {
                return Err(StorageError::RefusedScheme {
                    scheme: url.scheme().to_owned(),
                    reason: "an in-memory store forgets every object at restart",
                });
            }
            ObjectStoreScheme::Http => {
                return Err(StorageError::RefusedScheme {
                    scheme: url.scheme().to_owned(),
                    reason: "the object-store conformance suite does not cover an HTTP store",
                });
            }
            ObjectStoreScheme::Local => {
                let dir = url
                    .to_file_path()
                    .map_err(|()| invalid("not a local path".to_owned()))?;
                let dir = create_directory(&dir)?;
                let store =
                    DurableLocalFileSystem::new(&dir).map_err(|e| StorageError::Directory {
                        path: dir.display().to_string(),
                        source: std::io::Error::other(e),
                    })?;
                (Arc::new(store), Path::default())
            }
            _ => {
                let (store, root) = object_store::parse_url_opts(&url, &settings.options)
                    .map_err(|e| invalid(e.to_string()))?;
                (Arc::from(store), root)
            }
        };
        Ok(Self { store, root, url })
    }

    /// The store's URL, the default's resolved.
    pub fn url(&self) -> &Url {
        &self.url
    }

    /// The store itself, for an adapter that takes one, such as delta-rs's
    /// `with_storage_backend` once `oml-lake` is built (proposed ADR-0035 d10).
    pub fn store(&self) -> Arc<dyn ObjectStore> {
        Arc::clone(&self.store)
    }

    /// Stores `bytes` at `name` with `PutMode::Create`: a name already stored
    /// answers [`StorageError::AlreadyExists`] and keeps its bytes.
    pub async fn put(&self, name: &Path, bytes: Bytes) -> Result<(), StorageError> {
        self.store
            .put_opts(
                &self.full(name),
                PutPayload::from_bytes(bytes),
                PutMode::Create.into(),
            )
            .await
            .map(drop)
            .map_err(|e| StorageError::from_store(name, e))
    }

    /// Stores an upload part at `name` by plain put, the one write
    /// `PutMode::Create` does not cover: a retried part replaces its first
    /// (proposed ADR-0035 d9, d10).
    pub async fn put_part(&self, name: &Path, bytes: Bytes) -> Result<(), StorageError> {
        self.store
            .put(&self.full(name), PutPayload::from_bytes(bytes))
            .await
            .map(drop)
            .map_err(|e| StorageError::from_store(name, e))
    }

    /// The bytes stored at `name`.
    pub async fn get(&self, name: &Path) -> Result<Bytes, StorageError> {
        let found = self
            .store
            .get(&self.full(name))
            .await
            .map_err(|e| StorageError::from_store(name, e))?;
        found
            .bytes()
            .await
            .map_err(|e| StorageError::from_store(name, e))
    }

    /// Every name stored under `prefix`, relative to the root, sorted.
    pub async fn list(&self, prefix: &Path) -> Result<Vec<Path>, StorageError> {
        let full = self.full(prefix);
        let listed: Vec<_> = self
            .store
            .list(Some(&full))
            .try_collect()
            .await
            .map_err(|e| StorageError::from_store(prefix, e))?;
        let mut names: Vec<Path> = listed
            .into_iter()
            .filter_map(|meta| meta.location.prefix_match(&self.root).map(Path::from_iter))
            .collect();
        names.sort();
        Ok(names)
    }

    /// Deletes the object at `name`; a name already absent is not an error,
    /// so a sweep that repeats a delete succeeds.
    pub async fn delete(&self, name: &Path) -> Result<(), StorageError> {
        match self.store.delete(&self.full(name)).await {
            Ok(()) | Err(object_store::Error::NotFound { .. }) => Ok(()),
            Err(e) => Err(StorageError::from_store(name, e)),
        }
    }

    fn full(&self, name: &Path) -> Path {
        self.root.parts().chain(name.parts()).collect()
    }
}

/// Creates `dir` when missing and returns its canonical form.
fn create_directory(dir: &FsPath) -> Result<std::path::PathBuf, StorageError> {
    let fail = |source| StorageError::Directory {
        path: dir.display().to_string(),
        source,
    };
    std::fs::create_dir_all(dir).map_err(fail)?;
    std::fs::canonicalize(dir).map_err(fail)
}
