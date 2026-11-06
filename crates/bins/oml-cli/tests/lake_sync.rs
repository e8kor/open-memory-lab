//! `oml sync --adapter delta` (ADR-0062 d2, d5-d8, d11) over the committed test
//! lake, against a fake of the HTTP API that keeps versions and source checks.
#![cfg(feature = "lake-sync")]

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::PathBuf;

use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use oml_cli::client::CliError;
use oml_cli::lake::extract::{LakeItem, items};
use oml_cli::lake::read::{TableRead, read_lake};
use oml_cli::lake::run::{Options, execute};
use oml_cli::lake::uri::canonical;
use oml_cli::sync::api::{EdgeRow, ListQuery, Listed, Me, SyncApi};

const BASE: &str = "s3://acme-lake/warehouse";

fn fixture() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../testing/lake/fixture")
}

fn sha(lines: &[String]) -> String {
    format!(
        "sha256:{}",
        hex::encode(Sha256::digest(lines.join("\n").as_bytes()))
    )
}

fn items_of(rel: &str, at: Option<u64>) -> Vec<LakeItem> {
    let tables = read_lake(&fixture(), 3, at);
    let t: &TableRead = tables.iter().find(|t| t.rel == rel).expect("the table");
    let snap = t.outcome.as_ref().unwrap_or_else(|e| panic!("{rel}: {e}"));
    items(rel, &format!("{BASE}/{rel}"), snap)
}

fn shape(i: &LakeItem) -> (String, Option<String>, String, String) {
    (
        i.claim_key.clone(),
        i.fragment.clone(),
        i.text.clone(),
        i.hasher.to_owned(),
    )
}

#[test]
fn the_fixture_lake_is_two_tables_found_by_their_log() {
    let rels: Vec<String> = read_lake(&fixture(), 3, None)
        .into_iter()
        .map(|t| t.rel)
        .collect();
    assert_eq!(rels, ["events/clicks", "sales/orders"]);
}

#[test]
fn golden_claims_over_the_fixture_orders_table() {
    let got: Vec<_> = items_of("sales/orders", None).iter().map(shape).collect();
    let uri = format!("{BASE}/sales/orders");
    let table = format!(
        "Delta table orders at {uri}.\n\
         Purpose: Orders placed through the shop, one row per order\n\
         Owner: data-platform\n\
         Grain: one row per order\n\
         Freshness: daily by 06:00 UTC\n\
         Partitioned by: region"
    );
    let schema = concat!(
        r#"{"clustering_columns":[],"column_mapping":"name","columns":["#,
        r#"{"nullable":false,"path":"id","type":"long"},"#,
        r#"{"nullable":true,"path":"region","type":"string"},"#,
        r#"{"nullable":true,"path":"total_amount","type":"decimal(18,2)"},"#,
        r#"{"nullable":true,"path":"customer.email","type":"string"},"#,
        r#"{"nullable":true,"path":"customer.tier","type":"string"},"#,
        r#"{"nullable":true,"path":"channel","type":"string"}],"#,
        r#""partition_columns":["region"],"table":"orders"}"#
    );
    let col = |phys: &str, path: &str, meaning: &str| {
        (
            format!("delta/sales/orders/col/{phys}"),
            Some(format!("col={path}")),
            format!("Column {path} of table orders: {meaning}"),
            "oml-delta-col/1".to_owned(),
        )
    };
    let want = vec![
        (
            "delta/sales/orders/table".to_owned(),
            None,
            table,
            "oml-delta-table/1".to_owned(),
        ),
        (
            "delta/sales/orders/schema".to_owned(),
            None,
            schema.to_owned(),
            "oml-delta-schema/1".to_owned(),
        ),
        col("col-6f1a", "id", "Order identifier, unique per tenant"),
        col(
            "col-3c7d",
            "total_amount",
            "Order total in the sale currency, tax included",
        ),
        col("col-5a90", "customer.email", "Customer contact address"),
        col(
            "col-7d3e",
            "channel",
            "Sales channel the order came through",
        ),
    ];
    assert_eq!(got, want);
}

#[test]
fn golden_claims_over_the_fixture_clicks_table_with_no_column_mapping() {
    let got: Vec<_> = items_of("events/clicks", None).iter().map(shape).collect();
    let uri = format!("{BASE}/events/clicks");
    let table = format!(
        "Delta table clicks at {uri}.\n\
         Purpose: Page views and clicks of the public site\n\
         Owner: web-analytics\n\
         Clustered by: ts, page"
    );
    let schema = concat!(
        r#"{"clustering_columns":["ts","page"],"column_mapping":"none","columns":["#,
        r#"{"nullable":true,"path":"ts","type":"timestamp"},"#,
        r#"{"nullable":true,"path":"user_id","type":"string"},"#,
        r#"{"nullable":true,"path":"page","type":"string"}],"#,
        r#""partition_columns":[],"table":"clicks"}"#
    );
    let want = vec![
        (
            "delta/events/clicks/table".to_owned(),
            None,
            table,
            "oml-delta-table/1".to_owned(),
        ),
        (
            "delta/events/clicks/schema".to_owned(),
            None,
            schema.to_owned(),
            "oml-delta-schema/1".to_owned(),
        ),
        (
            "delta/events/clicks/col/user_id".to_owned(),
            Some("col=user_id".to_owned()),
            "Column user_id of table clicks: Visitor identifier, pseudonymous".to_owned(),
            "oml-delta-col/1".to_owned(),
        ),
    ];
    assert_eq!(got, want);
}

#[test]
fn each_hasher_covers_only_the_facts_its_claim_states() {
    let orders = items_of("sales/orders", None);
    let by = |k: &str| orders.iter().find(|i| i.claim_key.ends_with(k)).unwrap();
    let norm = |s: &str| s.split_whitespace().collect::<Vec<_>>().join(" ");
    assert_eq!(
        by("/table").hash,
        sha(&[
            "name orders".into(),
            format!(
                "desc {}",
                norm("Orders placed through the shop, one row per order")
            ),
            "owner data-platform".into(),
            "grain one row per order".into(),
            "freshness daily by 06:00 UTC".into(),
            "part region".into(),
        ])
    );
    assert_eq!(by("/schema").hash, sha(&[by("/schema").text.clone()]));
    assert_eq!(
        by("/col/col-3c7d").hash,
        sha(&[
            "col total_amount".into(),
            "comment Order total in the sale currency, tax included".into(),
        ])
    );
}

#[test]
fn an_append_changes_no_hash() {
    // Version 3 is the rename; 4 is an append of one data file.
    let a: Vec<_> = items_of("sales/orders", Some(3))
        .iter()
        .map(shape)
        .collect();
    let b: Vec<_> = items_of("sales/orders", Some(4))
        .iter()
        .map(shape)
        .collect();
    assert_eq!(a, b);
}

#[test]
fn no_data_stats_or_commit_info_content_is_ever_in_a_claim() {
    let mut all: Vec<LakeItem> = Vec::new();
    for rel in ["sales/orders", "events/clicks"] {
        for v in 0..=5 {
            all.extend(try_items(rel, v).unwrap_or_default());
        }
    }
    assert!(!all.is_empty());
    for i in &all {
        let payload = format!("{} {:?} {} {}", i.claim_key, i.fragment, i.text, i.hash);
        for sentinel in [
            "SENTINEL-USER",
            "SENTINEL-PARTITION-VALUE",
            "SENTINEL-STAT",
            "SENTINEL-SECRET",
            "SENTINEL-FILE",
            "secret-notebook",
            "part-0001",
            "numRecords",
            "fs.s3a",
            "/Users/",
        ] {
            assert!(!payload.contains(sentinel), "{sentinel} in {payload}");
        }
    }
}

fn try_items(rel: &str, v: u64) -> Option<Vec<LakeItem>> {
    let tables = read_lake(&fixture(), 3, Some(v));
    let t = tables.iter().find(|t| t.rel == rel)?;
    let snap = t.outcome.as_ref().ok()?;
    Some(items(rel, &format!("{BASE}/{rel}"), snap))
}

#[test]
fn table_keys_that_differ_only_in_case_never_collide() {
    let snap = read_lake(&fixture(), 3, None)
        .into_iter()
        .find(|t| t.rel == "sales/orders")
        .unwrap()
        .outcome
        .unwrap();
    let a = items("Sales/Orders", "s3://b/Sales/Orders", &snap);
    let b = items("sales/orders", "s3://b/sales/orders", &snap);
    assert_ne!(a[0].claim_key, b[0].claim_key);
    assert_eq!(b[0].claim_key, "delta/sales/orders/table");
    assert!(
        a[0].claim_key.starts_with("delta/sales/orders-"),
        "{}",
        a[0].claim_key
    );
    assert!(a.iter().chain(&b).all(|i| i.claim_key.len() <= 128));
}

#[test]
fn an_anchor_with_user_information_a_query_or_a_local_path_is_refused() {
    for bad in [
        "s3://key:secret@bucket/path",
        "https://user@host.example/path",
        "s3://bucket/path?versionId=1",
        "s3://bucket/path#frag",
        "/srv/lake/orders",
        "relative/lake",
        "ftp://host/path",
        "s3://Bad_Bucket/x",
        "s3://bucket/path/",
    ] {
        assert!(canonical(bad).is_err(), "{bad} was accepted");
    }
}

#[test]
fn locations_are_written_in_one_canonical_form() {
    assert_eq!(
        canonical("S3://Acme-Lake/Warehouse/Orders").unwrap(),
        "s3://acme-lake/Warehouse/Orders"
    );
    assert_eq!(canonical("gs://bucket/a/b").unwrap(), "gs://bucket/a/b");
    assert_eq!(
        canonical("https://Host.Example/lake/orders").unwrap(),
        "https://host.example/lake/orders"
    );
    assert_eq!(canonical("file:///srv/lake").unwrap(), "file:///srv/lake");
}

#[test]
fn adls_locations_are_written_as_their_https_endpoint() {
    assert_eq!(
        canonical("abfss://raw@acct.dfs.core.windows.net/sales/orders").unwrap(),
        "https://acct.dfs.core.windows.net/raw/sales/orders"
    );
}

// ---- the run, against a fake of the API ----

struct Held {
    artifact_id: String,
    claim_key: String,
    version: u64,
    state: String,
    retirement: bool,
    digest: String,
    anchor: Value,
    checks: Vec<Value>,
}

struct Fake {
    versions: RefCell<Vec<Held>>,
    writes: RefCell<Vec<(String, Value)>>,
    checks: RefCell<Vec<(String, Value)>>,
    /// Whether a write is promoted at once, as a reviewer who approves it.
    approve: RefCell<bool>,
}

impl Fake {
    fn new() -> Self {
        Self {
            versions: RefCell::new(Vec::new()),
            writes: RefCell::new(Vec::new()),
            checks: RefCell::new(Vec::new()),
            approve: RefCell::new(true),
        }
    }

    fn clear_calls(&self) {
        self.writes.borrow_mut().clear();
        self.checks.borrow_mut().clear();
    }

    fn tip(&self, key: &str) -> u64 {
        self.versions
            .borrow()
            .iter()
            .filter(|v| v.claim_key == key && v.state == "promoted")
            .map(|v| v.version)
            .max()
            .unwrap_or(0)
    }

    fn put(&self, key: &str, body: &Value, version: u64) -> Value {
        let id = format!("art-{key}-v{version}");
        let state = if *self.approve.borrow() {
            "promoted"
        } else {
            "held"
        };
        self.versions.borrow_mut().push(Held {
            artifact_id: id.clone(),
            claim_key: key.to_owned(),
            version,
            state: state.into(),
            retirement: body["retirement"] == true,
            digest: oml_cli::sync::hash::text_digest(body["body"].as_str().unwrap_or_default()),
            anchor: body["anchor"].clone(),
            checks: Vec::new(),
        });
        json!({"artifact_id": id, "state": state, "version": version})
    }
}

impl SyncApi for Fake {
    fn me(&self) -> Result<Me, CliError> {
        Ok(Me {
            key_id: "k-lake".into(),
            name: "lake-sync".into(),
        })
    }
    fn workarea_id(&self, workarea: &str) -> Result<String, CliError> {
        Ok(format!("wa-{workarea}"))
    }
    fn list(&self, q: &ListQuery) -> Result<Vec<Listed>, CliError> {
        let tip_of = |k: &str| self.tip(k);
        Ok(self
            .versions
            .borrow()
            .iter()
            .filter(|_| q.own)
            .map(|v| {
                Listed::of(&json!({
                    "artifact_id": v.artifact_id, "claim_key": v.claim_key, "version": v.version,
                    "tip": v.state == "promoted" && v.version == tip_of(&v.claim_key),
                    "state": v.state, "retirement": v.retirement,
                    "workarea_id": format!("wa-{}", q.workarea),
                    "plaintext_digest": v.digest, "anchor": v.anchor,
                    "source_checked": v.checks.last().map(|c| json!({
                        "rev": c["rev"], "at": "2026-11-06T00:00:00Z", "result": c["result"],
                    })),
                }))
            })
            .collect())
    }
    fn submit(&self, body: &Value, _key: &str) -> Result<Value, CliError> {
        self.writes
            .borrow_mut()
            .push(("submit".into(), body.clone()));
        let key = body["claim_key"].as_str().unwrap().to_owned();
        Ok(self.put(&key, body, self.tip(&key) + 1))
    }
    fn supersede(&self, artifact_id: &str, body: &Value, _key: &str) -> Result<Value, CliError> {
        self.writes
            .borrow_mut()
            .push((format!("supersede {artifact_id}"), body.clone()));
        let key = self
            .versions
            .borrow()
            .iter()
            .find(|v| v.artifact_id == artifact_id)
            .map(|v| v.claim_key.clone())
            .expect("the target");
        Ok(self.put(&key, body, self.tip(&key) + 1))
    }
    fn source_check(&self, artifact_id: &str, body: &Value, _key: &str) -> Result<Value, CliError> {
        self.checks
            .borrow_mut()
            .push((artifact_id.to_owned(), body.clone()));
        if let Some(v) = self
            .versions
            .borrow_mut()
            .iter_mut()
            .find(|v| v.artifact_id == artifact_id)
        {
            v.checks.push(body.clone());
        }
        Ok(json!({"result": body["result"]}))
    }
    fn claim_edges(&self, _: &str) -> Result<Vec<EdgeRow>, CliError> {
        Ok(Vec::new())
    }
    fn propose_edge(&self, _: &str, _: &str, _: &str, _: &str) -> Result<Value, CliError> {
        Err(CliError::local("no edges in a lake sync"))
    }
    fn ratify_edge(&self, _: &str, _: &str) -> Result<Value, CliError> {
        Err(CliError::local("no edges in a lake sync"))
    }
}

fn opts(at: Option<u64>) -> Options {
    Options {
        root: fixture(),
        uri_base: Some(BASE.into()),
        workarea: "lake".into(),
        max_writes: 200,
        depth: 3,
        at_version: at,
        ..Options::default()
    }
}

fn run(fake: &Fake, o: &Options) -> oml_cli::sync::run::Report {
    let mut sink = Vec::new();
    let report = execute(fake, o, &mut sink).unwrap();
    let printed = String::from_utf8(sink).unwrap();
    assert!(printed.contains("workarea  lake"), "{printed}");
    report
}

fn outcomes(report: &oml_cli::sync::run::Report) -> BTreeMap<String, String> {
    report.json["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| {
            (
                i["claim_key"].as_str().unwrap().to_owned(),
                i["outcome"].as_str().unwrap().to_owned(),
            )
        })
        .collect()
}

#[test]
fn a_first_run_writes_every_claim_with_a_uri_anchor_at_the_delta_version() {
    let fake = Fake::new();
    let r = run(&fake, &opts(None));
    assert_eq!(r.exit, 0);
    let out = outcomes(&r);
    assert_eq!(out.len(), 9, "{out:?}");
    assert!(out.values().all(|o| o == "new"));
    let writes = fake.writes.borrow();
    let (_, table) = writes
        .iter()
        .find(|(_, b)| b["claim_key"] == "delta/sales/orders/table")
        .unwrap();
    assert_eq!(table["workarea"], "lake");
    assert_eq!(
        table["anchor"],
        json!({
            "uri": "s3://acme-lake/warehouse/sales/orders",
            "rev": "5",
            "hash": items_of("sales/orders", None)[0].hash,
            "hasher": "oml-delta-table/1",
        })
    );
    let (_, col) = writes
        .iter()
        .find(|(_, b)| b["claim_key"] == "delta/sales/orders/col/col-5a90")
        .unwrap();
    assert_eq!(col["anchor"]["fragment"], "col=customer.email");
    assert_eq!(
        col["anchor"]["uri"],
        "s3://acme-lake/warehouse/sales/orders"
    );
}

#[test]
fn a_second_run_at_the_same_versions_writes_nothing() {
    let fake = Fake::new();
    run(&fake, &opts(None));
    fake.clear_calls();
    let r = run(&fake, &opts(None));
    assert!(outcomes(&r).values().all(|o| o == "unchanged"));
    assert!(fake.writes.borrow().is_empty());
}

#[test]
fn a_rename_under_column_mapping_is_one_supersession() {
    let fake = Fake::new();
    // Version 2: the column is still `amount`; version 3 renames it.
    run(&fake, &opts(Some(2)));
    fake.clear_calls();
    let r = run(&fake, &opts(Some(3)));
    let out = outcomes(&r);
    let changed: Vec<_> = out.iter().filter(|(_, o)| *o != "unchanged").collect();
    assert_eq!(
        changed,
        [
            (
                &"delta/sales/orders/col/col-3c7d".to_owned(),
                &"changed".to_owned()
            ),
            (
                &"delta/sales/orders/schema".to_owned(),
                &"changed".to_owned()
            ),
        ],
        "{out:?}"
    );
    let writes = fake.writes.borrow();
    let col: Vec<_> = writes
        .iter()
        .filter(|(_, b)| b["claim_key"].is_null() && b["anchor"]["fragment"].is_string())
        .collect();
    assert_eq!(col.len(), 1, "one column supersession");
    assert_eq!(col[0].1["anchor"]["fragment"], "col=total_amount");
    assert!(
        col[0].1["body"]
            .as_str()
            .unwrap()
            .starts_with("Column total_amount of table orders:")
    );
    // The old tip is marked changed by a source check before the write.
    assert!(
        fake.checks
            .borrow()
            .iter()
            .any(|(a, b)| a == "art-delta/sales/orders/col/col-3c7d-v1" && b["result"] == "changed")
    );
}

#[test]
fn an_unchanged_claim_is_checked_fresh_at_the_new_version_and_a_changed_one_changed() {
    let fake = Fake::new();
    run(&fake, &opts(Some(3)));
    fake.clear_calls();
    // Version 4 is an append: nothing a claim states moved, and the version did.
    let r = run(&fake, &opts(Some(4)));
    assert!(outcomes(&r).values().all(|o| o == "unchanged"));
    assert!(fake.writes.borrow().is_empty());
    {
        let checks = fake.checks.borrow();
        let orders: Vec<_> = checks
            .iter()
            .filter(|(a, _)| a.contains("sales/orders"))
            .collect();
        assert_eq!(orders.len(), 6, "{orders:?}");
        assert!(
            orders
                .iter()
                .all(|(_, b)| b["rev"] == "4" && b["result"] == "fresh")
        );
    }
    fake.clear_calls();
    // Version 5 edits a column's comment: that claim is stale, the others fresh.
    let r = run(&fake, &opts(Some(5)));
    let out = outcomes(&r);
    assert_eq!(out["delta/sales/orders/col/col-6f1a"], "changed");
    assert_eq!(out.values().filter(|o| *o == "changed").count(), 1);
    let checks = fake.checks.borrow();
    let stale = checks
        .iter()
        .find(|(a, _)| a == "art-delta/sales/orders/col/col-6f1a-v1")
        .unwrap();
    assert_eq!(stale.1["result"], "changed");
    assert_eq!(stale.1["rev"], "5");
}

#[test]
fn a_dry_run_sends_nothing_and_a_held_claim_is_pending() {
    let fake = Fake::new();
    *fake.approve.borrow_mut() = false;
    let mut o = opts(None);
    o.dry_run = true;
    let r = run(&fake, &o);
    assert!(fake.writes.borrow().is_empty());
    assert!(outcomes(&r).values().all(|o| o == "new"));
    o.dry_run = false;
    run(&fake, &o);
    fake.clear_calls();
    let r = run(&fake, &o);
    assert!(outcomes(&r).values().all(|o| o == "pending"));
    assert!(fake.writes.borrow().is_empty());
}

#[test]
fn a_dropped_comment_retires_its_column_claim_and_an_unreadable_table_retires_nothing() {
    let fake = Fake::new();
    run(&fake, &opts(Some(0)));
    // Version 0 has no `channel` column and a different set of claims than 2.
    fake.clear_calls();
    let r = run(&fake, &opts(Some(2)));
    let out = outcomes(&r);
    assert_eq!(out["delta/sales/orders/col/col-7d3e"], "new");
    fake.clear_calls();
    // Reading at version 0 again drops the `channel` claim: removed.
    let r = run(&fake, &opts(Some(0)));
    assert_eq!(outcomes(&r)["delta/sales/orders/col/col-7d3e"], "removed");
    // A lake whose only table is unsupported retires nothing and exits 1.
    let dir = tempfile_dir();
    let log = dir.join("sales/orders/_delta_log");
    std::fs::create_dir_all(&log).unwrap();
    std::fs::write(
        log.join("00000000000000000000.json"),
        r#"{"protocol":{"minReaderVersion":3,"minWriterVersion":7,"readerFeatures":["catalogManaged"],"writerFeatures":["catalogManaged"]}}"#,
    )
    .unwrap();
    let mut o = opts(None);
    o.root = dir;
    fake.clear_calls();
    let r = run(&fake, &o);
    assert_eq!(r.exit, 1);
    assert!(fake.writes.borrow().is_empty());
    assert!(
        r.json["tables"].as_array().unwrap().iter().any(|t| {
            t["table"] == "sales/orders"
                && t["state"] == "unsupported"
                && t["reason"].as_str().unwrap().contains("catalogManaged")
        }),
        "{}",
        r.json
    );
}

fn tempfile_dir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("oml-cli-lake-sync-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}
