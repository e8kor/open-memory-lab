//! The object-store port's checks of proposed ADR-0035 d12 step S1, against
//! the store `storage.url` picks (d10): the conformance suite runs on the
//! filesystem in a scratch directory and on S3 (MinIO in CI) when
//! `OML_TEST_S3_URL` names a bucket, and fails rather than skips in CI when it
//! does not.
//!
//! The S3 run takes `storage.options` from the standard variables
//! `AWS_ENDPOINT`, `AWS_ACCESS_KEY_ID`, `AWS_SECRET_ACCESS_KEY`, `AWS_REGION`
//! and `AWS_ALLOW_HTTP`, lower-cased into the object_store crate's keys.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};

use bytes::Bytes;
use oml_objectstore::object_store::path::Path;
use oml_objectstore::{
    DEFAULT_DATA_DIRECTORY, ObjectStorage, STORAGE_URL, StorageError, StorageSettings,
};

/// The variable naming the S3 bucket the suite also runs against.
const S3_URL_VARIABLE: &str = "OML_TEST_S3_URL";
const S3_OPTION_VARIABLES: [&str; 5] = [
    "AWS_ENDPOINT",
    "AWS_ACCESS_KEY_ID",
    "AWS_SECRET_ACCESS_KEY",
    "AWS_REGION",
    "AWS_ALLOW_HTTP",
];

/// A scratch directory under the system temp dir, removed on drop.
struct Scratch(std::path::PathBuf);

impl Scratch {
    fn new(tag: &str) -> Self {
        static N: AtomicUsize = AtomicUsize::new(0);
        let n = N.fetch_add(1, Ordering::SeqCst);
        let dir =
            std::env::temp_dir().join(format!("oml-objectstore-{tag}-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch directory");
        Self(dir)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn file_settings(dir: &std::path::Path) -> StorageSettings {
    let url = url::Url::from_directory_path(dir).expect("absolute scratch path");
    StorageSettings {
        url: Some(url.to_string()),
        options: BTreeMap::new(),
    }
}

/// The S3 settings from the environment, `None` when no bucket is named.
fn s3_settings() -> Option<StorageSettings> {
    let url = std::env::var(S3_URL_VARIABLE)
        .ok()
        .filter(|v| !v.is_empty())?;
    let options = S3_OPTION_VARIABLES
        .iter()
        .filter_map(|name| {
            std::env::var(name)
                .ok()
                .map(|value| (name.to_lowercase(), value))
        })
        .collect();
    Some(StorageSettings {
        url: Some(url),
        options,
    })
}

/// A prefix no other run shares, so repeated runs against one bucket never
/// meet each other's objects.
fn run_prefix(backend: &str) -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock after the epoch")
        .as_nanos();
    format!("conformance/{backend}-{}-{nanos}", std::process::id())
}

fn name(prefix: &str, rest: &str) -> Path {
    Path::parse(format!("{prefix}/{rest}")).expect("a valid object name")
}

/// The suite of d10: put, get, list, conditional put, the part path's
/// re-put, and delete as expiry uses it (twice, the second finding nothing).
async fn conformance(storage: &ObjectStorage, backend: &str) {
    let prefix = run_prefix(backend);
    let body = name(&prefix, "oml-quarantine/t/d/o");
    let other = name(&prefix, "oml-quarantine/t/d/p");
    let part = name(&prefix, "oml-quarantine/t/uploads/s/1");

    // put and get
    storage
        .put(&body, Bytes::from_static(b"first"))
        .await
        .unwrap_or_else(|e| panic!("{backend}: put: {e}"));
    assert_eq!(
        storage.get(&body).await.expect("get"),
        Bytes::from_static(b"first"),
        "{backend}: get returns the bytes put"
    );

    // conditional put: a second put at a stored object's name is refused
    // and the first bytes stay
    match storage.put(&body, Bytes::from_static(b"second")).await {
        Err(StorageError::AlreadyExists { name }) => {
            assert_eq!(name, body.to_string(), "{backend}: the refusal names it")
        }
        other => panic!("{backend}: a second put answered {other:?}"),
    }
    assert_eq!(
        storage.get(&body).await.expect("get"),
        Bytes::from_static(b"first"),
        "{backend}: the refused put left the first bytes"
    );

    // the part path: a retried part replaces its first
    storage
        .put_part(&part, Bytes::from_static(b"part-a"))
        .await
        .expect("put a part");
    storage
        .put_part(&part, Bytes::from_static(b"part-b"))
        .await
        .expect("re-put the part");
    assert_eq!(
        storage.get(&part).await.expect("get the part"),
        Bytes::from_static(b"part-b"),
        "{backend}: the retried part replaced its first"
    );

    // list: every name under the prefix, relative to the root, and no other
    storage
        .put(&other, Bytes::from_static(b"other"))
        .await
        .expect("put");
    let listed = storage
        .list(&Path::parse(&prefix).expect("prefix"))
        .await
        .expect("list");
    assert_eq!(
        listed,
        vec![body.clone(), other.clone(), part.clone()],
        "{backend}: list names every stored object, sorted"
    );
    let quarantine = storage
        .list(&name(&prefix, "oml-quarantine/t/d"))
        .await
        .expect("list");
    assert_eq!(quarantine, vec![body.clone(), other.clone()]);

    // a missing object answers NotFound
    let missing = name(&prefix, "absent");
    assert!(
        matches!(
            storage.get(&missing).await,
            Err(StorageError::NotFound { .. })
        ),
        "{backend}: a missing object is NotFound"
    );

    // delete, as expiry uses it: repeatable
    for target in [&body, &other, &part] {
        storage.delete(target).await.expect("delete");
        storage.delete(target).await.expect("a repeated delete");
        assert!(
            matches!(
                storage.get(target).await,
                Err(StorageError::NotFound { .. })
            ),
            "{backend}: a deleted object is gone"
        );
    }
    assert!(
        storage
            .list(&Path::parse(&prefix).expect("prefix"))
            .await
            .expect("list")
            .is_empty(),
        "{backend}: nothing is left under the prefix"
    );

    // a name once deleted takes a put again
    storage
        .put(&body, Bytes::from_static(b"again"))
        .await
        .expect("put after delete");
    storage.delete(&body).await.expect("delete");
}

#[tokio::test]
async fn the_filesystem_and_minio_backends_pass_one_object_store_conformance_suite() {
    let scratch = Scratch::new("conformance");
    let storage = ObjectStorage::open(&file_settings(&scratch.0), &scratch.0).expect("file store");
    conformance(&storage, "file").await;

    match s3_settings() {
        Some(settings) => {
            let storage = ObjectStorage::open(&settings, &scratch.0).expect("s3 store");
            conformance(&storage, "s3").await;
        }
        // GitHub Actions sets CI; there the MinIO run is required.
        None if std::env::var_os("CI").is_some() => panic!(
            "{S3_URL_VARIABLE} is unset in CI: the MinIO run of the conformance suite is required"
        ),
        None => eprintln!(
            "{S3_URL_VARIABLE} is unset: the conformance suite ran on the filesystem only"
        ),
    }
}

#[tokio::test]
async fn a_second_put_at_a_stored_objects_name_is_refused_while_a_retried_part_replaces_its_first()
{
    let scratch = Scratch::new("create");
    let storage = ObjectStorage::open(&file_settings(&scratch.0), &scratch.0).expect("file store");
    let object = Path::from("oml-quarantine/t/d/o");
    let part = Path::from("oml-quarantine/t/uploads/s/1");

    storage
        .put(&object, Bytes::from_static(b"one"))
        .await
        .expect("first put");
    assert!(matches!(
        storage.put(&object, Bytes::from_static(b"two")).await,
        Err(StorageError::AlreadyExists { .. })
    ));
    assert_eq!(
        std::fs::read(scratch.0.join("oml-quarantine/t/d/o")).expect("file"),
        b"one"
    );

    storage
        .put_part(&part, Bytes::from_static(b"one"))
        .await
        .expect("part");
    storage
        .put_part(&part, Bytes::from_static(b"two"))
        .await
        .expect("retried part");
    assert_eq!(
        std::fs::read(scratch.0.join("oml-quarantine/t/uploads/s/1")).expect("file"),
        b"two"
    );
}

#[tokio::test]
async fn an_unset_storage_url_stores_under_the_data_directory() {
    let scratch = Scratch::new("default");
    for url in [None, Some(String::new())] {
        let settings = StorageSettings {
            url,
            options: BTreeMap::new(),
        };
        let storage = ObjectStorage::open(&settings, &scratch.0).expect("default store");
        let data = std::fs::canonicalize(scratch.0.join(DEFAULT_DATA_DIRECTORY))
            .expect("the data directory exists once the store is open");
        assert_eq!(storage.url().scheme(), "file");
        assert_eq!(
            storage.url().to_file_path().expect("a file URL"),
            data,
            "the default URL is the data directory's"
        );
        let object = Path::from("oml-bodies/t/d/o");
        storage
            .put_part(&object, Bytes::from_static(b"bytes"))
            .await
            .expect("put");
        assert_eq!(
            std::fs::read(data.join("oml-bodies/t/d/o")).expect("the object's file"),
            b"bytes"
        );
    }
}

#[test]
fn a_memory_or_http_storage_url_is_refused_at_startup() {
    let scratch = Scratch::new("refused");
    for (url, scheme) in [
        ("memory:///", "memory"),
        ("http://127.0.0.1:9000/bucket", "http"),
        ("https://example.com/bucket", "https"),
    ] {
        let settings = StorageSettings {
            url: Some(url.to_owned()),
            options: BTreeMap::new(),
        };
        let err = ObjectStorage::open(&settings, &scratch.0).expect_err(url);
        assert!(
            matches!(&err, StorageError::RefusedScheme { scheme: s, .. } if s == scheme),
            "{url}: {err:?}"
        );
        assert!(
            err.to_string().contains(STORAGE_URL),
            "the refusal names the setting: {err}"
        );
    }
    let err = ObjectStorage::open(
        &StorageSettings {
            url: Some("not a url".to_owned()),
            options: BTreeMap::new(),
        },
        &scratch.0,
    )
    .expect_err("not a URL");
    assert!(matches!(err, StorageError::InvalidUrl { .. }), "{err:?}");
    assert!(err.to_string().contains(STORAGE_URL), "{err}");
}
