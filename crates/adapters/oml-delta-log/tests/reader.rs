//! The log reader (ADR-0062 d2-d4, d19) over the committed test lake and over
//! logs built in memory.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::PathBuf;

use oml_delta_log::{DirStore, Limits, LogError, LogStore, read_table, read_table_with};
use serde_json::{Value, json};

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../testing/lake/fixture")
}

/// A log held in memory.
#[derive(Default)]
struct Mem(BTreeMap<String, Vec<u8>>);

impl Mem {
    fn put(&mut self, name: &str, body: &str) -> &mut Self {
        self.0.insert(name.to_owned(), body.as_bytes().to_vec());
        self
    }
}

impl LogStore for Mem {
    fn list(&self) -> Result<Vec<String>, LogError> {
        Ok(self.0.keys().cloned().collect())
    }
    fn len(&self, name: &str) -> Result<u64, LogError> {
        self.get(name).map(|b| b.len() as u64)
    }
    fn get(&self, name: &str) -> Result<Vec<u8>, LogError> {
        self.0
            .get(name)
            .cloned()
            .ok_or_else(|| LogError::Unreadable(format!("{name} is absent")))
    }
    fn get_range(&self, name: &str, start: u64, len: u64) -> Result<Vec<u8>, LogError> {
        let b = self.get(name)?;
        let end = usize::try_from(start + len).unwrap().min(b.len());
        Ok(b[usize::try_from(start).unwrap()..end].to_vec())
    }
}

/// Records every name and byte range a read asks for.
struct Recording<'a, S: LogStore> {
    inner: &'a S,
    names: RefCell<Vec<String>>,
    ranges: RefCell<Vec<(String, u64, u64)>>,
}

impl<'a, S: LogStore> Recording<'a, S> {
    fn new(inner: &'a S) -> Self {
        Self {
            inner,
            names: RefCell::new(Vec::new()),
            ranges: RefCell::new(Vec::new()),
        }
    }
}

impl<S: LogStore> LogStore for Recording<'_, S> {
    fn list(&self) -> Result<Vec<String>, LogError> {
        self.inner.list()
    }
    fn len(&self, name: &str) -> Result<u64, LogError> {
        self.inner.len(name)
    }
    fn get(&self, name: &str) -> Result<Vec<u8>, LogError> {
        self.names.borrow_mut().push(name.to_owned());
        let body = self.inner.get(name)?;
        self.ranges
            .borrow_mut()
            .push((name.to_owned(), 0, body.len() as u64));
        Ok(body)
    }
    fn get_range(&self, name: &str, start: u64, len: u64) -> Result<Vec<u8>, LogError> {
        self.names.borrow_mut().push(name.to_owned());
        self.ranges.borrow_mut().push((name.to_owned(), start, len));
        self.inner.get_range(name, start, len)
    }
}

const PROTOCOL: &str = r#"{"protocol":{"minReaderVersion":1,"minWriterVersion":2}}"#;

fn meta(extra: &str) -> String {
    format!(
        r#"{{"metaData":{{"id":"t-1","name":"t","description":"a table","format":{{"provider":"parquet","options":{{}}}},"schemaString":"{{\"type\":\"struct\",\"fields\":[{{\"name\":\"a\",\"type\":\"long\",\"nullable\":true,\"metadata\":{{}}}}]}}","partitionColumns":[],"configuration":{{{extra}}}}}}}"#
    )
}

fn commit(lines: &[&str]) -> String {
    lines.join("\n") + "\n"
}

#[test]
fn the_reader_matches_the_delta_rs_oracle_at_every_fixture_version() {
    let oracle: Vec<Value> =
        serde_json::from_slice(&std::fs::read(fixture().join("oracle.json")).unwrap()).unwrap();
    assert!(oracle.len() >= 9, "the oracle covers both tables");
    for rec in oracle {
        let table = rec["table"].as_str().unwrap();
        let version = rec["version"].as_u64().unwrap();
        let store = DirStore::new(fixture().join(table));
        let got = read_table(&store, Some(version))
            .unwrap_or_else(|e| panic!("{table}@{version}: {e:?}"));
        assert_eq!(got.version, version, "{table}@{version}");
        assert_eq!(json!(got.name), rec["name"], "{table}@{version} name");
        assert_eq!(
            json!(got.description),
            rec["description"],
            "{table}@{version} description"
        );
        assert_eq!(
            json!(got.partition_columns),
            rec["partition_columns"],
            "{table}@{version} partitioning"
        );
        assert_eq!(
            json!(got.configuration.get("delta.columnMapping.mode")),
            rec["column_mapping_mode"],
            "{table}@{version} column mapping"
        );
        assert_eq!(got.schema_json(), rec["schema"], "{table}@{version} schema");
    }
}

#[test]
fn the_latest_version_is_the_default_and_a_checkpoint_plus_commits_reach_it() {
    let orders = read_table(&DirStore::new(fixture().join("sales/orders")), None).unwrap();
    assert_eq!(orders.version, 5);
    let id = orders.schema.iter().find(|f| f.name == "id").unwrap();
    assert_eq!(
        id.metadata["comment"],
        json!("Order identifier, unique per tenant")
    );
}

#[test]
fn clustering_columns_are_read_from_the_log_and_from_a_checkpoint_alone() {
    // events/clicks keeps no JSON commit below its checkpoint at version 2.
    for v in [2, 3, 4] {
        let s = read_table(&DirStore::new(fixture().join("events/clicks")), Some(v)).unwrap();
        assert_eq!(
            s.clustering_columns,
            vec![vec!["ts".to_owned()], vec!["page".to_owned()]],
            "version {v}"
        );
    }
}

#[test]
fn no_object_outside_delta_log_is_ever_opened() {
    let store = DirStore::new(fixture().join("sales/orders"));
    let rec = Recording::new(&store);
    read_table(&rec, None).unwrap();
    let names = rec.names.borrow();
    assert!(!names.is_empty());
    for n in names.iter() {
        assert!(
            !n.contains('/') && !n.contains(".."),
            "{n} is not a file directly under _delta_log"
        );
    }
}

#[test]
fn commit_info_add_and_remove_are_never_decoded() {
    // Each is valid JSON of a shape that would fail to decode as its action,
    // and carries a value no snapshot may hold.
    let mut m = Mem::default();
    m.put(
        "00000000000000000000.json",
        &commit(&[
            PROTOCOL,
            &meta(""),
            r#"{"add":12}"#,
            r#"{"remove":"SENTINEL-REMOVE"}"#,
            r#"{"commitInfo":["SENTINEL-USER","SENTINEL-NOTEBOOK"]}"#,
            r#"{"txn":null}"#,
            r#"{"cdc":true}"#,
            r#"{"sidecar":{"path":"SENTINEL-SIDECAR"}}"#,
        ]),
    );
    let s = read_table(&m, None).unwrap();
    let shown = format!("{s:?}");
    assert!(!shown.contains("SENTINEL"), "{shown}");
    assert_eq!(s.version, 0);
}

#[test]
fn only_d9_properties_are_read() {
    let secret = r#""fs.s3a.secret.key":"SENTINEL-SECRET","oml.owner":"data-platform","delta.columnMapping.mode":"name","delta.logRetentionDuration":"interval 30 days""#;
    let mut m = Mem::default();
    m.put(
        "00000000000000000000.json",
        &commit(&[PROTOCOL, &meta(secret)]),
    );
    let s = read_table(&m, None).unwrap();
    let keys: Vec<&str> = s.configuration.keys().map(String::as_str).collect();
    assert_eq!(keys, ["delta.columnMapping.mode", "oml.owner"]);
    // The fixture's checkpoint carries the same secret in its metaData row.
    let o = read_table(&DirStore::new(fixture().join("sales/orders")), Some(3)).unwrap();
    assert!(!format!("{o:?}").contains("SENTINEL"));
    assert!(o.configuration.contains_key("oml.owner"));
    assert!(!o.configuration.contains_key("fs.s3a.secret.key"));
}

/// The byte ranges of one top-level column's chunks in a parquet file.
fn column_ranges(bytes: &[u8], top: &str) -> Vec<(u64, u64)> {
    use parquet::file::reader::{FileReader, SerializedFileReader};
    let r = SerializedFileReader::new(bytes::Bytes::copy_from_slice(bytes)).unwrap();
    let md = r.metadata();
    let mut out = Vec::new();
    for rg in md.row_groups() {
        for c in rg.columns() {
            if c.column_path().parts()[0] == top {
                let (s, l) = c.byte_range();
                out.push((s, l));
            }
        }
    }
    out
}

#[test]
fn a_checkpoint_is_read_by_projection_without_its_add_column() {
    let table = fixture().join("events/clicks");
    let cp =
        std::fs::read(table.join("_delta_log/00000000000000000002.checkpoint.parquet")).unwrap();
    let add = column_ranges(&cp, "add");
    assert!(!add.is_empty(), "the fixture's checkpoint carries add");
    let store = DirStore::new(&table);
    let rec = Recording::new(&store);
    let s = read_table(&rec, Some(2)).unwrap();
    assert_eq!(s.version, 2);
    let mut fetched = 0;
    for (name, start, len) in rec.ranges.borrow().iter() {
        if !name.ends_with(".checkpoint.parquet") {
            continue;
        }
        fetched += len;
        for (a, al) in &add {
            assert!(
                start + len <= *a || *start >= a + al,
                "{name} bytes {start}+{len} overlap the add column at {a}+{al}"
            );
        }
    }
    assert!(fetched > 0 && fetched < cp.len() as u64);
}

#[test]
fn an_unknown_reader_feature_is_unsupported_by_name() {
    let mut m = Mem::default();
    m.put(
        "00000000000000000000.json",
        &commit(&[
            r#"{"protocol":{"minReaderVersion":3,"minWriterVersion":7,"readerFeatures":["columnMapping","fancyNewThing"],"writerFeatures":["columnMapping"]}}"#,
            &meta(""),
        ]),
    );
    match read_table(&m, None) {
        Err(LogError::Unsupported(what)) => assert!(what.contains("fancyNewThing"), "{what}"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_catalog_managed_table_is_refused() {
    let mut m = Mem::default();
    m.put(
        "00000000000000000000.json",
        &commit(&[
            r#"{"protocol":{"minReaderVersion":3,"minWriterVersion":7,"readerFeatures":["catalogManaged"],"writerFeatures":["catalogManaged"]}}"#,
            &meta(""),
        ]),
    );
    match read_table(&m, None) {
        Err(LogError::Unsupported(what)) => assert!(what.contains("catalogManaged"), "{what}"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_log_gap_is_unreadable() {
    let mut m = Mem::default();
    m.put("00000000000000000000.json", &commit(&[PROTOCOL, &meta("")]));
    m.put("00000000000000000002.json", &commit(&[]));
    assert!(matches!(read_table(&m, None), Err(LogError::Unreadable(_))));
}

#[test]
fn a_last_checkpoint_naming_a_missing_file_is_unreadable() {
    let mut m = Mem::default();
    m.put("00000000000000000000.json", &commit(&[PROTOCOL, &meta("")]));
    m.put("00000000000000000001.json", &commit(&[]));
    m.put("_last_checkpoint", r#"{"version":1,"size":2}"#);
    assert!(matches!(read_table(&m, None), Err(LogError::Unreadable(_))));
}

#[test]
fn a_checkpoint_that_does_not_parse_is_unreadable_and_never_skipped() {
    let mut m = Mem::default();
    m.put("00000000000000000000.json", &commit(&[PROTOCOL, &meta("")]));
    m.put(
        "00000000000000000001.checkpoint.parquet",
        "not parquet at all",
    );
    m.put("00000000000000000002.json", &commit(&[]));
    assert!(matches!(read_table(&m, None), Err(LogError::Unreadable(_))));
}

#[test]
fn a_version_the_log_does_not_reach_is_unreadable() {
    let mut m = Mem::default();
    m.put("00000000000000000000.json", &commit(&[PROTOCOL, &meta("")]));
    assert!(matches!(
        read_table(&m, Some(7)),
        Err(LogError::Unreadable(_))
    ));
}

fn scratch(test: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("oml-delta-log-{test}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn unreadable_with(r: Result<impl std::fmt::Debug, LogError>, words: &[&str]) {
    match r {
        Err(LogError::Unreadable(why)) => {
            for w in words {
                assert!(why.contains(w), "{why:?} does not name {w:?}");
            }
        }
        other => panic!("expected unreadable naming {words:?}, got {other:?}"),
    }
}

#[test]
fn the_store_opens_no_name_that_leaves_the_log_on_any_platform() {
    let dir = scratch("traversal");
    let table = dir.join("t");
    std::fs::create_dir_all(table.join("_delta_log")).unwrap();
    std::fs::create_dir_all(table.join("region=EU")).unwrap();
    // Files a guard-less join would open: beside the log, in a partition
    // directory, and one named by its absolute path.
    std::fs::write(table.join("x.parquet"), b"outside").unwrap();
    std::fs::write(table.join("region=EU/part-0.parquet"), b"data").unwrap();
    let absolute = table.join("x.parquet");
    let store = DirStore::new(&table);
    let absolute = absolute.to_str().unwrap();
    for bad in [
        "../x.parquet",
        "..\\x.parquet",
        "region=EU/part-0.parquet",
        "region=EU\\part-0.parquet",
        absolute,
        "a/b",
        "..",
        ".",
        "",
        "x\0y",
    ] {
        let expect = ["is not a file directly under _delta_log"];
        unreadable_with(store.get(bad), &expect);
        unreadable_with(store.get_range(bad, 0, 1), &expect);
        unreadable_with(store.len(bad), &expect);
    }
}

#[cfg(unix)]
#[test]
fn a_symlink_is_never_followed_by_the_store() {
    use std::os::unix::fs::symlink;
    let dir = scratch("symlinks");
    let real = dir.join("real");
    std::fs::create_dir_all(real.join("_delta_log")).unwrap();
    std::fs::write(
        real.join("_delta_log/00000000000000000000.json"),
        commit(&[PROTOCOL, &meta("")]),
    )
    .unwrap();
    // A table whose whole _delta_log is a link to another directory's.
    let linked = dir.join("linked");
    std::fs::create_dir_all(&linked).unwrap();
    symlink(real.join("_delta_log"), linked.join("_delta_log")).unwrap();
    unreadable_with(read_table(&DirStore::new(&linked), None), &["symlink"]);
    assert!(read_table(&DirStore::new(&real), None).is_ok());
    // A commit that is a link to a file elsewhere reads as a missing commit.
    let log = dir.join("t/_delta_log");
    std::fs::create_dir_all(&log).unwrap();
    std::fs::write(
        log.join("00000000000000000000.json"),
        commit(&[PROTOCOL, &meta("")]),
    )
    .unwrap();
    std::fs::write(dir.join("elsewhere.json"), commit(&[])).unwrap();
    symlink(
        dir.join("elsewhere.json"),
        log.join("00000000000000000001.json"),
    )
    .unwrap();
    let store = DirStore::new(dir.join("t"));
    assert!(!store.list().unwrap().iter().any(|n| n.ends_with("1.json")));
    unreadable_with(store.get("00000000000000000001.json"), &["symlink"]);
    unreadable_with(store.len("00000000000000000001.json"), &["symlink"]);
    unreadable_with(
        store.get_range("00000000000000000001.json", 0, 1),
        &["symlink"],
    );
    assert_eq!(read_table(&store, None).unwrap().version, 0);
}

/// A tiny deterministic generator, so a failing name can be replayed.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn digits(&mut self, n: usize) -> String {
        (0..n)
            .map(|_| char::from(b'0' + (self.next() % 10) as u8))
            .collect()
    }
    fn number(&mut self) -> String {
        match self.next() % 6 {
            0 => "9".repeat(1 + (self.next() % 40) as usize),
            1 => "18446744073709551615".into(),
            2 => "18446744073709551616".into(),
            3 => "0".repeat(1 + (self.next() % 30) as usize),
            _ => {
                let n = 1 + (self.next() % 25) as usize;
                self.digits(n)
            }
        }
    }
}

#[test]
fn a_crafted_log_file_name_neither_allocates_nor_aborts_nor_overflows() {
    let base = || {
        let mut m = Mem::default();
        m.put("00000000000000000000.json", &commit(&[PROTOCOL, &meta("")]));
        m
    };
    // The name that asked `vec![None; 9999999999999]` for 240 TB.
    let mut m = base();
    m.put(
        "00000000000000000000.checkpoint.0000000001.9999999999999.parquet",
        "x",
    );
    assert_eq!(read_table(&m, None).unwrap().version, 0);
    // A version at the top of u64: `v + 1` must not wrap or panic.
    let mut m = base();
    m.put("18446744073709551615.checkpoint.parquet", "x");
    assert!(read_table(&m, None).is_err());
    let mut m = base();
    m.put("18446744073709551615.json", "");
    assert!(read_table(&m, None).is_err());
    // A seeded sweep over names of the three checkpoint shapes.
    let mut rng = Rng(0x9e37_79b9_7f4a_7c15);
    for _ in 0..4000 {
        let mut m = base();
        let v = format!("{:0>20}", rng.number());
        let name = match rng.next() % 5 {
            0 => format!("{v}.checkpoint.{}.{}.parquet", rng.number(), rng.number()),
            1 => format!("{v}.checkpoint.parquet"),
            2 => format!("{v}.checkpoint.{}.parquet", rng.digits(8)),
            3 => format!("{v}.json"),
            _ => format!(
                "{}.checkpoint.{}.{}.parquet",
                rng.number(),
                rng.number(),
                rng.number()
            ),
        };
        m.put(&name, "x");
        let _ = read_table(&m, None);
    }
}

fn clicks_log() -> Mem {
    let mut m = Mem::default();
    let log = fixture().join("events/clicks/_delta_log");
    for e in std::fs::read_dir(log).unwrap() {
        let e = e.unwrap();
        m.0.insert(
            e.file_name().into_string().unwrap(),
            std::fs::read(e.path()).unwrap(),
        );
    }
    m
}

#[test]
fn a_corrupt_checkpoint_is_unreadable_for_its_table_and_never_a_panic() {
    let clean = clicks_log();
    assert!(read_table(&clean, Some(2)).is_ok());
    let name = "00000000000000000002.checkpoint.parquet";
    let len = clean.0[name].len();
    let mut panicked = Vec::new();
    for i in 0..len {
        for mask in [0xFF_u8, 0x80, 0x01] {
            let mut m = Mem(clean.0.clone());
            m.0.get_mut(name).unwrap()[i] ^= mask;
            let run =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| read_table(&m, Some(2))));
            if run.is_err() {
                panicked.push((i, mask));
            }
        }
    }
    assert!(
        panicked.is_empty(),
        "the reader panicked on flips {panicked:?}"
    );
}

struct Claimed {
    name: String,
    len: u64,
    tail: Option<Vec<u8>>,
}
impl LogStore for Claimed {
    fn list(&self) -> Result<Vec<String>, LogError> {
        Ok(vec![self.name.clone()])
    }
    fn len(&self, _: &str) -> Result<u64, LogError> {
        Ok(self.len)
    }
    fn get(&self, name: &str) -> Result<Vec<u8>, LogError> {
        panic!("{name} was read whole though it is over its cap");
    }
    fn get_range(&self, name: &str, start: u64, len: u64) -> Result<Vec<u8>, LogError> {
        match &self.tail {
            Some(t) if start == self.len - 8 && len == 8 => Ok(t.clone()),
            _ => panic!("{name} bytes {start}+{len} were read though the footer is over its cap"),
        }
    }
}

#[test]
fn a_commit_of_300_megabytes_is_refused_before_it_is_read() {
    let s = Claimed {
        name: "00000000000000000000.json".into(),
        len: 300_000_000,
        tail: None,
    };
    unreadable_with(
        read_table(&s, None),
        &["00000000000000000000.json", "commit", "cap"],
    );
}

#[test]
fn a_commit_line_over_its_cap_is_refused_by_name() {
    let mut m = Mem::default();
    m.put(
        "00000000000000000000.json",
        &commit(&[
            PROTOCOL,
            &meta(""),
            &format!(r#"{{"add":{{"path":"{}"}}}}"#, "a".repeat(5000)),
        ]),
    );
    assert!(read_table(&m, None).is_ok());
    let tight = Limits {
        commit_line: 1000,
        ..Limits::default()
    };
    unreadable_with(read_table_with(&m, None, &tight), &["line 3", "cap"]);
}

#[test]
fn a_checkpoint_footer_over_its_cap_is_refused_before_it_is_read() {
    let mut tail = 100_000_000_u32.to_le_bytes().to_vec();
    tail.extend_from_slice(b"PAR1");
    let s = Claimed {
        name: "00000000000000000000.checkpoint.parquet".into(),
        len: 1 << 31,
        tail: Some(tail),
    };
    unreadable_with(read_table(&s, None), &["footer", "cap"]);
}

#[test]
fn a_checkpoint_whose_projected_columns_are_over_the_cap_is_refused() {
    let tight = Limits {
        checkpoint_bytes: 100,
        ..Limits::default()
    };
    unreadable_with(
        read_table_with(&clicks_log(), Some(2), &tight),
        &["checkpoint", "projected", "cap"],
    );
}

#[test]
fn a_schema_of_too_many_columns_is_refused_by_name() {
    let tight = Limits {
        columns: 0,
        ..Limits::default()
    };
    let mut m = Mem::default();
    m.put("00000000000000000000.json", &commit(&[PROTOCOL, &meta("")]));
    unreadable_with(read_table_with(&m, None, &tight), &["columns", "cap"]);
}

#[test]
fn a_log_of_too_many_files_is_refused_by_name() {
    let mut m = Mem::default();
    m.put("00000000000000000000.json", &commit(&[PROTOCOL, &meta("")]));
    m.put("00000000000000000001.json", &commit(&[]));
    m.put("00000000000000000002.json", &commit(&[]));
    let tight = Limits {
        files: 2,
        ..Limits::default()
    };
    unreadable_with(read_table_with(&m, None, &tight), &["files", "cap"]);
    assert!(read_table(&m, None).is_ok());
}
