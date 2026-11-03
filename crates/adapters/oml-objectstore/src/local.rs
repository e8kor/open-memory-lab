//! The file store: object_store's `LocalFileSystem`, made durable.
//!
//! object_store 0.13.2's `LocalFileSystem` syncs nothing (its `src/local.rs`
//! calls no `sync_all`) and has no `with_fsync`, which 0.14 adds, so until the
//! pin reaches 0.14 this wrapper syncs each file a put, copy, rename or
//! multipart completion writes, and every directory from its parent up to the
//! store's root, before it returns (proposed ADR-0035 d10, "Two obligations on
//! every write"). From 0.14 it is `LocalFileSystem::with_fsync(true)`.
//! Reads, lists and deletes pass through unchanged.

use std::fmt;
use std::io;
use std::path::{Path as FsPath, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use futures_util::stream::BoxStream;
use object_store::local::LocalFileSystem;
use object_store::path::Path;
use object_store::{
    CopyOptions, GetOptions, GetResult, ListResult, MultipartUpload, ObjectMeta, ObjectStore,
    PutMultipartOptions, PutOptions, PutPayload, PutResult, RenameOptions, Result, UploadPart,
};

/// How a written file and its directories reach stable storage; the seam the
/// crate's own tests record through.
pub(crate) trait Syncer: fmt::Debug + Send + Sync + 'static {
    fn sync_file(&self, path: &FsPath) -> io::Result<()>;
    fn sync_dir(&self, path: &FsPath) -> io::Result<()>;
}

/// `fsync` through `File::sync_all`.
#[derive(Debug)]
struct FsSync;

impl Syncer for FsSync {
    fn sync_file(&self, path: &FsPath) -> io::Result<()> {
        std::fs::File::open(path)?.sync_all()
    }

    #[cfg(unix)]
    fn sync_dir(&self, path: &FsPath) -> io::Result<()> {
        std::fs::File::open(path)?.sync_all()
    }

    /// A directory cannot be opened as a file off unix; NTFS journals its
    /// metadata itself.
    #[cfg(not(unix))]
    fn sync_dir(&self, _path: &FsPath) -> io::Result<()> {
        Ok(())
    }
}

/// What a write needs to make itself durable: the root it stops at and the
/// syncer.
#[derive(Debug, Clone)]
struct Durability {
    root: PathBuf,
    syncer: Arc<dyn Syncer>,
}

impl Durability {
    /// Syncs `file`, then each directory from its parent up to the root, on
    /// the blocking pool.
    async fn sync(&self, file: PathBuf) -> Result<()> {
        let this = self.clone();
        tokio::task::spawn_blocking(move || this.sync_now(&file))
            .await
            .map_err(|e| generic(io::Error::other(e)))?
    }

    fn sync_now(&self, file: &FsPath) -> Result<()> {
        self.syncer.sync_file(file).map_err(generic)?;
        let mut dir = file.parent();
        while let Some(d) = dir {
            if !d.starts_with(&self.root) {
                break;
            }
            self.syncer.sync_dir(d).map_err(generic)?;
            if d == self.root {
                break;
            }
            dir = d.parent();
        }
        Ok(())
    }
}

fn generic(source: io::Error) -> object_store::Error {
    object_store::Error::Generic {
        store: "DurableLocalFileSystem",
        source: Box::new(source),
    }
}

/// object_store's `LocalFileSystem` over one root directory, returning from
/// a write only once the written file and its directory entries are synced
/// (proposed ADR-0035 d10).
#[derive(Debug)]
pub struct DurableLocalFileSystem {
    inner: LocalFileSystem,
    durability: Durability,
}

impl DurableLocalFileSystem {
    /// The store rooted at `root`, an existing directory.
    pub fn new(root: &FsPath) -> Result<Self> {
        Self::with_syncer(root, Arc::new(FsSync))
    }

    pub(crate) fn with_syncer(root: &FsPath, syncer: Arc<dyn Syncer>) -> Result<Self> {
        let root = std::fs::canonicalize(root).map_err(generic)?;
        let inner = LocalFileSystem::new_with_prefix(&root)?;
        Ok(Self {
            inner,
            durability: Durability { root, syncer },
        })
    }

    async fn after_write(&self, location: &Path) -> Result<()> {
        let file = self.inner.path_to_filesystem(location)?;
        self.durability.sync(file).await
    }
}

impl fmt::Display for DurableLocalFileSystem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "DurableLocalFileSystem({})",
            self.durability.root.display()
        )
    }
}

#[async_trait]
impl ObjectStore for DurableLocalFileSystem {
    async fn put_opts(
        &self,
        location: &Path,
        payload: PutPayload,
        opts: PutOptions,
    ) -> Result<PutResult> {
        let result = self.inner.put_opts(location, payload, opts).await?;
        self.after_write(location).await?;
        Ok(result)
    }

    async fn put_multipart_opts(
        &self,
        location: &Path,
        opts: PutMultipartOptions,
    ) -> Result<Box<dyn MultipartUpload>> {
        let file = self.inner.path_to_filesystem(location)?;
        let inner = self.inner.put_multipart_opts(location, opts).await?;
        Ok(Box::new(DurableUpload {
            inner,
            file,
            durability: self.durability.clone(),
        }))
    }

    async fn get_opts(&self, location: &Path, options: GetOptions) -> Result<GetResult> {
        self.inner.get_opts(location, options).await
    }

    async fn get_ranges(
        &self,
        location: &Path,
        ranges: &[std::ops::Range<u64>],
    ) -> Result<Vec<bytes::Bytes>> {
        self.inner.get_ranges(location, ranges).await
    }

    fn delete_stream(
        &self,
        locations: BoxStream<'static, Result<Path>>,
    ) -> BoxStream<'static, Result<Path>> {
        self.inner.delete_stream(locations)
    }

    fn list(&self, prefix: Option<&Path>) -> BoxStream<'static, Result<ObjectMeta>> {
        self.inner.list(prefix)
    }

    fn list_with_offset(
        &self,
        prefix: Option<&Path>,
        offset: &Path,
    ) -> BoxStream<'static, Result<ObjectMeta>> {
        self.inner.list_with_offset(prefix, offset)
    }

    async fn list_with_delimiter(&self, prefix: Option<&Path>) -> Result<ListResult> {
        self.inner.list_with_delimiter(prefix).await
    }

    async fn copy_opts(&self, from: &Path, to: &Path, options: CopyOptions) -> Result<()> {
        self.inner.copy_opts(from, to, options).await?;
        self.after_write(to).await
    }

    async fn rename_opts(&self, from: &Path, to: &Path, options: RenameOptions) -> Result<()> {
        self.inner.rename_opts(from, to, options).await?;
        self.after_write(to).await
    }
}

/// A multipart upload whose completion returns once the assembled file and
/// its directories are synced.
#[derive(Debug)]
struct DurableUpload {
    inner: Box<dyn MultipartUpload>,
    file: PathBuf,
    durability: Durability,
}

#[async_trait]
impl MultipartUpload for DurableUpload {
    fn put_part(&mut self, data: PutPayload) -> UploadPart {
        self.inner.put_part(data)
    }

    async fn complete(&mut self) -> Result<PutResult> {
        let result = self.inner.complete().await?;
        self.durability.sync(self.file.clone()).await?;
        Ok(result)
    }

    async fn abort(&mut self) -> Result<()> {
        self.inner.abort().await
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use object_store::{ObjectStoreExt, PutMode};

    use super::*;

    /// Records each sync, and syncs for real, so the order is observable.
    #[derive(Debug, Default)]
    struct Recorder(Mutex<Vec<(&'static str, PathBuf)>>);

    impl Recorder {
        fn take(&self) -> Vec<(&'static str, PathBuf)> {
            std::mem::take(&mut self.0.lock().expect("recorder"))
        }
    }

    impl Syncer for Recorder {
        fn sync_file(&self, path: &FsPath) -> io::Result<()> {
            self.0.lock().expect("recorder").push(("file", path.into()));
            FsSync.sync_file(path)
        }

        fn sync_dir(&self, path: &FsPath) -> io::Result<()> {
            self.0.lock().expect("recorder").push(("dir", path.into()));
            FsSync.sync_dir(path)
        }
    }

    fn scratch() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("oml-objectstore-sync-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch");
        std::fs::canonicalize(dir).expect("scratch")
    }

    /// The file, then each directory from its parent up to the root.
    fn expected(root: &FsPath, name: &str) -> Vec<(&'static str, PathBuf)> {
        let file = root.join(name);
        let mut want = vec![("file", file.clone())];
        let mut dir = file.parent();
        while let Some(d) = dir {
            want.push(("dir", d.to_path_buf()));
            if d == root {
                break;
            }
            dir = d.parent();
        }
        want
    }

    #[tokio::test]
    async fn the_file_store_syncs_each_written_file_and_its_directory_before_it_returns() {
        let root = scratch();
        let recorder = Arc::new(Recorder::default());
        let store =
            DurableLocalFileSystem::with_syncer(&root, recorder.clone()).expect("file store");

        // a create put
        let object = Path::from("oml-quarantine/t/d/o");
        store
            .put_opts(
                &object,
                PutPayload::from_static(b"x"),
                PutMode::Create.into(),
            )
            .await
            .expect("put");
        assert_eq!(recorder.take(), expected(&root, "oml-quarantine/t/d/o"));

        // a part's plain put
        let part = Path::from("oml-quarantine/t/uploads/s/1");
        store
            .put(&part, PutPayload::from_static(b"p"))
            .await
            .expect("part");
        assert_eq!(
            recorder.take(),
            expected(&root, "oml-quarantine/t/uploads/s/1")
        );

        // a multipart completion syncs the assembled file, not before
        let assembled = Path::from("oml-quarantine/t/d/f");
        let mut upload = store.put_multipart(&assembled).await.expect("multipart");
        upload
            .put_part(PutPayload::from_static(b"frame"))
            .await
            .expect("part");
        assert!(recorder.take().is_empty(), "nothing is complete yet");
        upload.complete().await.expect("complete");
        assert_eq!(recorder.take(), expected(&root, "oml-quarantine/t/d/f"));

        // the promotion copy
        let body = Path::from("oml-bodies/t/d/o");
        store.copy(&object, &body).await.expect("copy");
        assert_eq!(recorder.take(), expected(&root, "oml-bodies/t/d/o"));

        // reads and deletes sync nothing
        store.get(&body).await.expect("get");
        store.delete(&part).await.expect("delete");
        assert!(recorder.take().is_empty());

        std::fs::remove_dir_all(&root).expect("cleanup");
    }
}
