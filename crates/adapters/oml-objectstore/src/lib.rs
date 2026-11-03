//! `oml-objectstore` — the one object-store port over the Rust `object_store`
//! crate (proposed ADR-0035 d10): every stored object, and once the lake is
//! built every lake table, lives in the one store `storage.url` picks.
//!
//! - **One URL.** [`ObjectStorage::open`] classifies `storage.url` with the
//!   crate's own [`ObjectStoreScheme::parse`] and builds the store with
//!   [`object_store::parse_url_opts`] from it and `storage.options`, the
//!   crate's endpoint and credential keys, each checked against the scheme's
//!   own key enum first, so a misspelt key is refused rather than dropped;
//!   [`ObjectStorage::store`] hands the same store, rooted at the URL's path
//!   on every scheme, to any other adapter, as `oml-lake` will take it.
//! - **The default.** Unset or empty, `storage.url` is the `file://` URL of
//!   [`DEFAULT_DATA_DIRECTORY`] in the working directory, so a basic install
//!   runs no storage service.
//! - **The file store syncs.** A `file://` URL builds a
//!   [`DurableLocalFileSystem`], which returns from a write only once its file
//!   and directory entries are synced (d10, "Two obligations on every write"),
//!   and on Linux is refused on an NFS or SMB mount (ADR-0023 d3).
//! - **Put-if-absent.** Every put but a part's is `PutMode::Create`, so a
//!   stored object's name is written once ([`ObjectStorage::put`]); a part is a
//!   plain put a retry replaces ([`ObjectStorage::put_part`]), as do the
//!   promotion copy ([`ObjectStorage::copy`]) and the multipart assembly
//!   ([`ObjectStorage::assemble`]). No write asks the store for a
//!   compare-and-set: those stay in Postgres. [`ObjectStorage::self_test`] is
//!   ADR-0016 d5's scratch-key proof of put-if-absent, which a writer runs
//!   before it reports ready.
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
use object_store::aws::AmazonS3ConfigKey;
use object_store::azure::AzureConfigKey;
use object_store::gcp::GoogleConfigKey;
use object_store::path::Path;
use object_store::prefix::PrefixStore;
use object_store::{
    ObjectStore, ObjectStoreExt, ObjectStoreScheme, PutMode, PutPayload, WriteMultipart,
};
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
    /// A `storage.options` key the URL's scheme does not know, or any key
    /// with a `file://` URL, which takes none (proposed ADR-0035 d10).
    #[error("storage.options key `{key}` is refused for the `{scheme}` scheme: {reason}")]
    RefusedOption {
        key: String,
        scheme: String,
        reason: String,
    },
    /// The `file://` directory is on a network filesystem, where
    /// put-if-absent's atomicity is not promised (ADR-0023 d3).
    #[error(
        "storage.url's directory `{path}` is on a {filesystem} mount, which is refused: \
         put-if-absent is atomic on a local filesystem only"
    )]
    RefusedFilesystem {
        path: String,
        filesystem: &'static str,
    },
    /// The store did not prove put-if-absent on a scratch key (ADR-0016 d5;
    /// proposed ADR-0035 d10).
    #[error("storage.url `{url}` failed the put-if-absent self-test: {reason}")]
    SelfTest { url: String, reason: String },
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

/// The prefix [`ObjectStorage::self_test`] writes its scratch keys under.
pub const SELF_TEST_PREFIX: &str = "oml-self-test";

/// The parts an assembly keeps in flight at once, bounding its memory to this
/// many 5 MiB chunks.
const ASSEMBLY_PARTS_IN_FLIGHT: usize = 4;

/// The one store `storage.url` picks, with names relative to the URL's path.
#[derive(Debug, Clone)]
pub struct ObjectStorage {
    /// Rooted at the URL's path on every scheme.
    store: Arc<dyn ObjectStore>,
    url: Url,
}

impl ObjectStorage {
    /// Opens the store `settings` name, the default under `working_dir` when
    /// `storage.url` is unset or empty. A `file://` directory is created when
    /// missing and, on Linux, refused on an NFS or SMB mount; `memory` and
    /// `http`/`https` are refused, as is any `storage.options` key the scheme
    /// does not know (proposed ADR-0035 d10; ADR-0023 d3). A writer runs
    /// [`Self::self_test`] before it reports ready.
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
        check_options(&scheme, &url, &settings.options)?;
        let store: Arc<dyn ObjectStore> = match scheme {
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
                refuse_network_filesystem(&dir)?;
                let store =
                    DurableLocalFileSystem::new(&dir).map_err(|e| StorageError::Directory {
                        path: dir.display().to_string(),
                        source: std::io::Error::other(e),
                    })?;
                Arc::new(store)
            }
            _ => {
                let (store, root) = object_store::parse_url_opts(&url, &settings.options)
                    .map_err(|e| invalid(e.to_string()))?;
                if root.as_ref().is_empty() {
                    Arc::from(store)
                } else {
                    Arc::new(PrefixStore::new(store, root))
                }
            }
        };
        Ok(Self { store, url })
    }

    /// The store's URL, the default's resolved.
    pub fn url(&self) -> &Url {
        &self.url
    }

    /// The store itself, rooted at the URL's path on every scheme, so a name
    /// means the same object here as through [`Self::put`]: for an adapter
    /// that takes one, such as delta-rs's `with_storage_backend` once
    /// `oml-lake` is built (proposed ADR-0035 d10).
    pub fn store(&self) -> Arc<dyn ObjectStore> {
        Arc::clone(&self.store)
    }

    /// Stores `bytes` at `name` with `PutMode::Create`: a name already stored
    /// answers [`StorageError::AlreadyExists`] and keeps its bytes.
    pub async fn put(&self, name: &Path, bytes: Bytes) -> Result<(), StorageError> {
        self.store
            .put_opts(name, PutPayload::from_bytes(bytes), PutMode::Create.into())
            .await
            .map(drop)
            .map_err(|e| StorageError::from_store(name, e))
    }

    /// Stores an upload part at `name` by plain put, the one write
    /// `PutMode::Create` does not cover: a retried part replaces its first
    /// (proposed ADR-0035 d9, d10).
    pub async fn put_part(&self, name: &Path, bytes: Bytes) -> Result<(), StorageError> {
        self.store
            .put(name, PutPayload::from_bytes(bytes))
            .await
            .map(drop)
            .map_err(|e| StorageError::from_store(name, e))
    }

    /// The bytes stored at `name`.
    pub async fn get(&self, name: &Path) -> Result<Bytes, StorageError> {
        let found = self
            .store
            .get(name)
            .await
            .map_err(|e| StorageError::from_store(name, e))?;
        found
            .bytes()
            .await
            .map_err(|e| StorageError::from_store(name, e))
    }

    /// Every name stored under `prefix`, relative to the root, sorted.
    pub async fn list(&self, prefix: &Path) -> Result<Vec<Path>, StorageError> {
        let listed: Vec<_> = self
            .store
            .list(Some(prefix))
            .try_collect()
            .await
            .map_err(|e| StorageError::from_store(prefix, e))?;
        let mut names: Vec<Path> = listed.into_iter().map(|meta| meta.location).collect();
        names.sort();
        Ok(names)
    }

    /// Deletes the object at `name`; a name already absent is not an error,
    /// so a sweep that repeats a delete succeeds.
    pub async fn delete(&self, name: &Path) -> Result<(), StorageError> {
        match self.store.delete(name).await {
            Ok(()) | Err(object_store::Error::NotFound { .. }) => Ok(()),
            Err(e) => Err(StorageError::from_store(name, e)),
        }
    }

    /// Copies the object at `from` to `to` by plain copy, replacing what `to`
    /// holds: the promotion copy to `oml-bodies`, which an interrupted move
    /// repeats with the same ciphertext (proposed ADR-0035 d9, d10). A missing
    /// source answers [`StorageError::NotFound`] naming it.
    pub async fn copy(&self, from: &Path, to: &Path) -> Result<(), StorageError> {
        self.store.copy(from, to).await.map_err(|e| match e {
            object_store::Error::NotFound { .. } => StorageError::NotFound {
                name: from.to_string(),
            },
            other => StorageError::from_store(to, other),
        })
    }

    /// Streams the objects at `parts`, in order, into one object at `to`
    /// through one multipart upload, then deletes the parts: completion of an
    /// upload session (proposed ADR-0035 d9). The upload sends 5 MiB chunks,
    /// S3's minimum part size, whatever the parts' sizes; like a part's put,
    /// the completion replaces what `to` holds (d10). A failed assembly aborts
    /// its upload and keeps every part.
    pub async fn assemble(&self, parts: &[Path], to: &Path) -> Result<(), StorageError> {
        let upload = self
            .store
            .put_multipart(to)
            .await
            .map_err(|e| StorageError::from_store(to, e))?;
        let mut writer = WriteMultipart::new(upload);
        let streamed = async {
            for part in parts {
                let mut chunks = self
                    .store
                    .get(part)
                    .await
                    .map_err(|e| StorageError::from_store(part, e))?
                    .into_stream();
                while let Some(chunk) = chunks
                    .try_next()
                    .await
                    .map_err(|e| StorageError::from_store(part, e))?
                {
                    writer
                        .wait_for_capacity(ASSEMBLY_PARTS_IN_FLIGHT)
                        .await
                        .map_err(|e| StorageError::from_store(to, e))?;
                    writer.put(chunk);
                }
            }
            Ok(())
        }
        .await;
        if let Err(e) = streamed {
            // The abort's own failure leaves only an upload the store expires.
            let _ = writer.abort().await;
            return Err(e);
        }
        writer
            .finish()
            .await
            .map_err(|e| StorageError::from_store(to, e))?;
        for part in parts {
            self.delete(part).await?;
        }
        Ok(())
    }

    /// ADR-0016 d5's scratch-key self-test, run on every scheme (proposed
    /// ADR-0035 d10): a `PutMode::Create` put of a fresh key under
    /// [`SELF_TEST_PREFIX`] must succeed and a second must answer
    /// `AlreadyExists`, else [`StorageError::SelfTest`] names `storage.url`.
    /// The scratch key is deleted either way. A writer refuses readiness
    /// while it fails.
    pub async fn self_test(&self) -> Result<(), StorageError> {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        let scratch = Path::from_iter([
            SELF_TEST_PREFIX.to_owned(),
            format!("{}-{nanos}", std::process::id()),
        ]);
        let proved = self.prove_put_if_absent(&scratch).await;
        let deleted = self.delete(&scratch).await;
        proved?;
        deleted
    }

    async fn prove_put_if_absent(&self, scratch: &Path) -> Result<(), StorageError> {
        let failed = |reason: String| StorageError::SelfTest {
            url: self.url.to_string(),
            reason,
        };
        self.put(scratch, Bytes::from_static(b"first"))
            .await
            .map_err(|e| failed(format!("a put-if-absent of a fresh key failed: {e}")))?;
        match self.put(scratch, Bytes::from_static(b"second")).await {
            Err(StorageError::AlreadyExists { .. }) => Ok(()),
            Ok(()) => Err(failed(
                "a second put-if-absent at the same key succeeded".to_owned(),
            )),
            Err(e) => Err(failed(format!(
                "a second put-if-absent at the same key did not answer AlreadyExists: {e}"
            ))),
        }
    }
}

/// Refuses any `storage.options` key `scheme` does not know, parsed as
/// [`object_store::parse_url_opts`] parses it (lower-cased, through the
/// scheme's own key enum), which drops an unknown key without a word; and
/// any key with a `file://` URL, which takes none (proposed ADR-0035 d10).
fn check_options(
    scheme: &ObjectStoreScheme,
    url: &Url,
    options: &BTreeMap<String, String>,
) -> Result<(), StorageError> {
    for key in options.keys() {
        let lowered = key.to_ascii_lowercase();
        let known = match scheme {
            ObjectStoreScheme::AmazonS3 => lowered
                .parse::<AmazonS3ConfigKey>()
                .map(drop)
                .map_err(|e| e.to_string()),
            ObjectStoreScheme::MicrosoftAzure => lowered
                .parse::<AzureConfigKey>()
                .map(drop)
                .map_err(|e| e.to_string()),
            ObjectStoreScheme::GoogleCloudStorage => lowered
                .parse::<GoogleConfigKey>()
                .map(drop)
                .map_err(|e| e.to_string()),
            _ => Err("this scheme takes no options".to_owned()),
        };
        known.map_err(|reason| StorageError::RefusedOption {
            key: key.clone(),
            scheme: url.scheme().to_owned(),
            reason,
        })?;
    }
    Ok(())
}

/// The network filesystem a `statfs(2)` `f_type` magic names, if it is one
/// ADR-0023 d3 refuses: NFS, SMB, CIFS or SMB2 (the magics of `statfs(2)`).
#[cfg(target_os = "linux")]
fn network_filesystem(magic: u64) -> Option<&'static str> {
    match magic & 0xFFFF_FFFF {
        0x6969 => Some("NFS"),
        0x517B => Some("SMB"),
        0xFF53_4D42 => Some("CIFS"),
        0xFE53_4D42 => Some("SMB2"),
        _ => None,
    }
}

/// Refuses a data directory on an NFS or SMB mount (ADR-0023 d3). Off Linux
/// the put-if-absent self-test is the check.
#[cfg(target_os = "linux")]
fn refuse_network_filesystem(dir: &FsPath) -> Result<(), StorageError> {
    let stat = rustix::fs::statfs(dir).map_err(|e| StorageError::Directory {
        path: dir.display().to_string(),
        source: e.into(),
    })?;
    #[allow(clippy::unnecessary_cast)] // `f_type`'s width differs by architecture
    let magic = stat.f_type as u64;
    match network_filesystem(magic) {
        Some(filesystem) => Err(StorageError::RefusedFilesystem {
            path: dir.display().to_string(),
            filesystem,
        }),
        None => Ok(()),
    }
}

#[cfg(not(target_os = "linux"))]
fn refuse_network_filesystem(_dir: &FsPath) -> Result<(), StorageError> {
    Ok(())
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

#[cfg(test)]
mod tests {
    use async_trait::async_trait;
    use futures_util::stream::BoxStream;
    use object_store::memory::InMemory;
    use object_store::{
        CopyOptions, GetOptions, GetResult, ListResult, MultipartUpload, ObjectMeta,
        PutMultipartOptions, PutOptions, PutResult,
    };

    use super::*;

    /// How a stand-in store answers a `PutMode::Create` put.
    #[derive(Debug, Clone, Copy)]
    enum Create {
        /// As S3 with `aws_conditional_put=disabled` answers.
        NotImplemented,
        /// Overwrites, as a store that ignores the mode would.
        Overwrites,
    }

    /// An in-memory store whose create puts break put-if-absent one way.
    #[derive(Debug)]
    struct NoPutIfAbsent {
        inner: InMemory,
        create: Create,
    }

    impl std::fmt::Display for NoPutIfAbsent {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(f, "NoPutIfAbsent({:?})", self.create)
        }
    }

    #[async_trait]
    impl ObjectStore for NoPutIfAbsent {
        async fn put_opts(
            &self,
            location: &Path,
            payload: PutPayload,
            opts: PutOptions,
        ) -> object_store::Result<PutResult> {
            match (&opts.mode, self.create) {
                (PutMode::Create, Create::NotImplemented) => {
                    Err(object_store::Error::NotImplemented {
                        operation: "PutMode::Create".to_owned(),
                        implementer: "NoPutIfAbsent".to_owned(),
                    })
                }
                (PutMode::Create, Create::Overwrites) => self.inner.put(location, payload).await,
                _ => self.inner.put_opts(location, payload, opts).await,
            }
        }

        async fn put_multipart_opts(
            &self,
            location: &Path,
            opts: PutMultipartOptions,
        ) -> object_store::Result<Box<dyn MultipartUpload>> {
            self.inner.put_multipart_opts(location, opts).await
        }

        async fn get_opts(
            &self,
            location: &Path,
            options: GetOptions,
        ) -> object_store::Result<GetResult> {
            self.inner.get_opts(location, options).await
        }

        fn delete_stream(
            &self,
            locations: BoxStream<'static, object_store::Result<Path>>,
        ) -> BoxStream<'static, object_store::Result<Path>> {
            self.inner.delete_stream(locations)
        }

        fn list(
            &self,
            prefix: Option<&Path>,
        ) -> BoxStream<'static, object_store::Result<ObjectMeta>> {
            self.inner.list(prefix)
        }

        async fn list_with_delimiter(
            &self,
            prefix: Option<&Path>,
        ) -> object_store::Result<ListResult> {
            self.inner.list_with_delimiter(prefix).await
        }

        async fn copy_opts(
            &self,
            from: &Path,
            to: &Path,
            options: CopyOptions,
        ) -> object_store::Result<()> {
            self.inner.copy_opts(from, to, options).await
        }
    }

    #[tokio::test]
    async fn a_store_without_put_if_absent_fails_the_self_test_naming_the_setting() {
        for create in [Create::NotImplemented, Create::Overwrites] {
            let storage = ObjectStorage {
                store: Arc::new(NoPutIfAbsent {
                    inner: InMemory::new(),
                    create,
                }),
                url: Url::parse("s3://bucket/root").expect("url"),
            };
            let err = storage.self_test().await.expect_err("no put-if-absent");
            assert!(
                matches!(err, StorageError::SelfTest { .. }),
                "{create:?}: {err:?}"
            );
            assert!(err.to_string().contains(STORAGE_URL), "{create:?}: {err}");
            assert!(
                storage
                    .list(&Path::default())
                    .await
                    .expect("list")
                    .is_empty(),
                "{create:?}: a failed self-test still deletes its scratch key"
            );
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn a_file_storage_url_on_an_nfs_or_smb_mount_is_refused_naming_the_setting() {
        // statfs(2) f_type magics: NFS, SMB, CIFS and SMB2.
        for (magic, filesystem) in [
            (0x6969, "NFS"),
            (0x517B, "SMB"),
            (0xFF53_4D42, "CIFS"),
            (0xFE53_4D42, "SMB2"),
        ] {
            assert_eq!(network_filesystem(magic), Some(filesystem), "{magic:#x}");
            let err = StorageError::RefusedFilesystem {
                path: "/mnt/share/oml-data".to_owned(),
                filesystem,
            };
            let message = err.to_string();
            assert!(
                message.contains(STORAGE_URL) && message.contains(filesystem),
                "{message}"
            );
        }
        // ext4, xfs, btrfs, tmpfs
        for magic in [0xEF53, 0x5846_5342, 0x9123_683E, 0x0102_1994] {
            assert_eq!(network_filesystem(magic), None, "{magic:#x}");
        }
        // the scratch directory the other tests store under is local
        let local = std::env::temp_dir();
        refuse_network_filesystem(&local).expect("the temp directory is a local filesystem");
    }
}
