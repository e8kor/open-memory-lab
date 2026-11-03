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
    DEFAULT_DATA_DIRECTORY, ObjectStorage, STORAGE_OPTIONS, STORAGE_URL, StorageError,
    StorageSettings,
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

/// `len` bytes of `fill`, so each assembled part is told apart.
fn filled(fill: u8, len: usize) -> Bytes {
    Bytes::from(vec![fill; len])
}

/// The suite of d10: put, get, list, copy, conditional put, the part path
/// (put a part, re-put it, assemble) and delete as expiry uses it (twice, the
/// second finding nothing).
async fn conformance(storage: &ObjectStorage, backend: &str) {
    let prefix = run_prefix(backend);
    let body = name(&prefix, "oml-quarantine/t/d/o");
    let other = name(&prefix, "oml-quarantine/t/d/p");
    let part = name(&prefix, "oml-quarantine/t/uploads/s/1");
    let promoted = name(&prefix, "oml-bodies/t/d/o");
    let file = name(&prefix, "oml-quarantine/t/d/f");

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

    // copy: the promotion copy to oml-bodies, a plain copy an interrupted
    // move repeats; the source stays
    for attempt in ["copy", "the repeated copy"] {
        storage
            .copy(&body, &promoted)
            .await
            .unwrap_or_else(|e| panic!("{backend}: {attempt}: {e}"));
        assert_eq!(
            storage.get(&promoted).await.expect("get the copy"),
            Bytes::from_static(b"first"),
            "{backend}: {attempt} holds the source's bytes"
        );
    }
    assert_eq!(
        storage.get(&body).await.expect("get the source"),
        Bytes::from_static(b"first"),
        "{backend}: the copy left its source"
    );
    let absent = name(&prefix, "oml-quarantine/t/d/absent");
    match storage.copy(&absent, &promoted).await {
        Err(StorageError::NotFound { name }) => {
            assert_eq!(
                name,
                absent.to_string(),
                "{backend}: the refusal names the source"
            )
        }
        other => panic!("{backend}: copying a missing object answered {other:?}"),
    }
    storage.delete(&promoted).await.expect("delete the copy");

    // assemble: several parts streamed in order into one object through one
    // multipart upload, then the parts deleted. Two parts of 3 MiB and a
    // third of 1 MiB cross S3's 5 MiB minimum part size, so the upload sends
    // more than one part.
    let parts: Vec<Path> = (1..=3)
        .map(|n| name(&prefix, &format!("oml-quarantine/t/uploads/a/{n}")))
        .collect();
    let contents = [
        filled(b'a', 3 << 20),
        filled(b'b', 3 << 20),
        filled(b'c', 1 << 20),
    ];
    for (part, bytes) in parts.iter().zip(&contents) {
        storage
            .put_part(part, bytes.clone())
            .await
            .expect("put a part to assemble");
    }
    storage
        .assemble(&parts, &file)
        .await
        .unwrap_or_else(|e| panic!("{backend}: assemble: {e}"));
    assert_eq!(
        storage.get(&file).await.expect("get the assembled object"),
        contents.concat(),
        "{backend}: the assembled object is its parts in order"
    );
    for part in &parts {
        assert!(
            matches!(storage.get(part).await, Err(StorageError::NotFound { .. })),
            "{backend}: assembly deleted the part {part}"
        );
    }
    storage
        .delete(&file)
        .await
        .expect("delete the assembled object");

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

    if let Some(settings) = required_s3_settings("the conformance suite") {
        let storage = ObjectStorage::open(&settings, &scratch.0).expect("s3 store");
        conformance(&storage, "s3").await;
    }
}

/// The S3 run's settings, or `None` off CI when no bucket is named; in CI an
/// unnamed bucket fails the test rather than skipping it.
fn required_s3_settings(test: &str) -> Option<StorageSettings> {
    match s3_settings() {
        Some(settings) => Some(settings),
        // GitHub Actions sets CI; there the MinIO run is required.
        None if std::env::var_os("CI").is_some() => {
            panic!("{S3_URL_VARIABLE} is unset in CI: the MinIO run of {test} is required")
        }
        None => {
            eprintln!("{S3_URL_VARIABLE} is unset: {test} ran on the filesystem only");
            None
        }
    }
}

/// `settings` with `segment` appended to its URL's path.
fn beneath(settings: &StorageSettings, segment: &str) -> StorageSettings {
    let url = settings.url.as_deref().expect("a URL");
    StorageSettings {
        url: Some(format!("{}/{segment}", url.trim_end_matches('/'))),
        options: settings.options.clone(),
    }
}

/// Opens `settings` beneath a fresh sub-path, writes once through `put` and
/// once through `store()`, and checks through a store opened at the parent
/// that both landed under the sub-path.
async fn rooted(parent_settings: &StorageSettings, working_dir: &std::path::Path, backend: &str) {
    use oml_objectstore::object_store::{ObjectStoreExt, PutPayload};

    let sub = run_prefix(backend).replace('/', "-");
    let parent = ObjectStorage::open(parent_settings, working_dir).expect("parent store");
    let storage =
        ObjectStorage::open(&beneath(parent_settings, &sub), working_dir).expect("rooted store");
    let through_put = Path::from("oml-bodies/t/d/put");
    let through_store = Path::from("oml-bodies/t/d/store");
    storage
        .put(&through_put, Bytes::from_static(b"put"))
        .await
        .expect("put");
    storage
        .store()
        .put(&through_store, PutPayload::from_static(b"store"))
        .await
        .expect("put through store()");

    let sub_path = Path::from(sub.as_str());
    assert_eq!(
        parent.list(&sub_path).await.expect("list the parent"),
        vec![
            name(&sub, "oml-bodies/t/d/put"),
            name(&sub, "oml-bodies/t/d/store")
        ],
        "{backend}: put and store() both write beneath the URL's path"
    );
    assert_eq!(
        storage.list(&Path::default()).await.expect("list"),
        vec![through_put.clone(), through_store.clone()],
        "{backend}: the rooted store names its objects relative to its root"
    );
    for n in [&through_put, &through_store] {
        storage.delete(n).await.expect("delete");
    }
}

#[tokio::test]
async fn the_store_and_its_puts_are_rooted_at_the_urls_path_on_every_backend() {
    let scratch = Scratch::new("rooted");
    rooted(&file_settings(&scratch.0), &scratch.0, "file").await;
    if let Some(settings) = required_s3_settings("the rooted store's check") {
        rooted(&settings, &scratch.0, "s3").await;
    }
}

#[tokio::test]
async fn the_self_test_proves_put_if_absent_and_leaves_no_scratch_key() {
    let scratch = Scratch::new("self-test");
    let mut backends = vec![("file", file_settings(&scratch.0))];
    backends.extend(required_s3_settings("the self-test").map(|s| {
        (
            "s3",
            beneath(&s, &run_prefix("self-test").replace('/', "-")),
        )
    }));
    for (backend, settings) in backends {
        let storage = ObjectStorage::open(&settings, &scratch.0).expect("store");
        storage
            .self_test()
            .await
            .unwrap_or_else(|e| panic!("{backend}: the self-test: {e}"));
        assert!(
            storage
                .list(&Path::default())
                .await
                .expect("list")
                .is_empty(),
            "{backend}: the self-test left nothing behind"
        );
    }
}

#[tokio::test]
async fn an_s3_store_without_conditional_put_fails_the_self_test_naming_the_setting() {
    let Some(settings) = required_s3_settings("the conditional-put refusal") else {
        return;
    };
    let mut settings = beneath(
        &settings,
        &run_prefix("no-conditional-put").replace('/', "-"),
    );
    settings
        .options
        .insert("aws_conditional_put".to_owned(), "disabled".to_owned());
    let storage = ObjectStorage::open(&settings, std::path::Path::new(".")).expect("store");
    let err = storage.self_test().await.expect_err("no put-if-absent");
    assert!(matches!(err, StorageError::SelfTest { .. }), "{err:?}");
    assert!(err.to_string().contains(STORAGE_URL), "{err}");
}

#[test]
fn an_unknown_storage_option_is_refused_naming_the_setting() {
    let scratch = Scratch::new("options");
    for (url, key) in [
        ("s3://bucket/root", "aws_endpont"),
        ("az://container/root", "aws_endpoint"),
        ("gs://bucket/root", "google_service_acount"),
        ("file:///", "aws_endpoint"),
    ] {
        let url = if url == "file:///" {
            url::Url::from_directory_path(&scratch.0)
                .expect("absolute")
                .to_string()
        } else {
            url.to_owned()
        };
        let settings = StorageSettings {
            url: Some(url.clone()),
            options: BTreeMap::from([(key.to_owned(), "x".to_owned())]),
        };
        let err = ObjectStorage::open(&settings, &scratch.0).expect_err(&url);
        assert!(
            matches!(&err, StorageError::RefusedOption { key: k, .. } if k == key),
            "{url}: {err:?}"
        );
        let message = err.to_string();
        assert!(
            message.contains(STORAGE_OPTIONS) && message.contains(key),
            "{url}: the refusal names the setting and the key: {message}"
        );
    }
    // the keys the CI run passes are known
    let settings = StorageSettings {
        url: Some("s3://bucket".to_owned()),
        options: S3_OPTION_VARIABLES
            .iter()
            .zip([
                "http://127.0.0.1:9000",
                "oml",
                "oml_dev_only",
                "us-east-1",
                "true",
            ])
            .map(|(v, value)| (v.to_lowercase(), value.to_owned()))
            .collect(),
    };
    ObjectStorage::open(&settings, &scratch.0).expect("the standard keys open");
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
