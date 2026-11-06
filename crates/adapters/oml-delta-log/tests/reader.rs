//! The log reader (ADR-0062 d2-d4, d19) over the committed test lake and over
//! logs built in memory.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::PathBuf;

use oml_delta_log::{DirStore, LogError, LogStore, read_table};
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
    // The store itself refuses a name that leaves the log's directory, so a
    // data file or a sibling table is out of reach by construction.
    for bad in [
        "../x.parquet",
        "region=EU/part-0.parquet",
        "/etc/passwd",
        "a/b",
    ] {
        assert!(store.get(bad).is_err(), "{bad} was opened");
        assert!(store.get_range(bad, 0, 1).is_err(), "{bad} was opened");
        assert!(store.len(bad).is_err(), "{bad} was opened");
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
