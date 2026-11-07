//! Generates the test lake under `fixture/` and its `oracle.json` (ADR-0062 d3,
//! d19): `cargo run --manifest-path testing/lake/Cargo.toml -- testing/lake/fixture`.
//!
//! The logs are written by hand, because a rename under column mapping is a
//! metaData action delta-rs does not write; the checkpoints and the oracle are
//! delta-rs's, which reads the hand-written log and writes the checkpoint
//! parquet. The sentinels below are strings no claim may ever carry.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use url::Url;

const SENTINEL_USER: &str = "SENTINEL-USER-eugene";
const SENTINEL_VALUE: &str = "SENTINEL-PARTITION-VALUE-eu";
const SENTINEL_STATS: &str = "SENTINEL-STAT-9917";
const SENTINEL_SECRET: &str = "SENTINEL-SECRET-TOKEN";
const SENTINEL_FILE: &str = "SENTINEL-FILE-part-0001";

fn field(name: &str, ty: Value, nullable: bool, comment: Option<&str>, id: u32, phys: Option<&str>) -> Value {
    let mut md = serde_json::Map::new();
    if let Some(c) = comment {
        md.insert("comment".into(), json!(c));
    }
    if let Some(p) = phys {
        md.insert("delta.columnMapping.id".into(), json!(id));
        md.insert("delta.columnMapping.physicalName".into(), json!(p));
    }
    json!({"name": name, "type": ty, "nullable": nullable, "metadata": md})
}

fn schema(fields: Vec<Value>) -> String {
    serde_json::to_string(&json!({"type": "struct", "fields": fields})).unwrap()
}

fn orders_schema(amount: &str, id_comment: &str, with_channel: bool) -> String {
    let mut f = vec![
        field("id", json!("long"), false, Some(id_comment), 1, Some("col-6f1a")),
        field("region", json!("string"), true, None, 2, Some("col-02b9")),
        field(
            amount,
            json!("decimal(18,2)"),
            true,
            Some("Order total in the sale currency, tax included"),
            3,
            Some("col-3c7d"),
        ),
        field(
            "customer",
            json!({"type": "struct", "fields": [
                field("email", json!("string"), true, Some("Customer contact address"), 5, Some("col-5a90")),
                field("tier", json!("string"), true, None, 6, Some("col-6b44")),
            ]}),
            true,
            None,
            4,
            Some("col-4e21"),
        ),
    ];
    if with_channel {
        f.push(field("channel", json!("string"), true, Some("Sales channel the order came through"), 7, Some("col-7d3e")));
    }
    schema(f)
}

fn meta(schema_string: String, partitions: &[&str], max_id: u32, description: Option<&str>) -> Value {
    json!({"metaData": {
        "id": "5f0e1c6a-0b0a-4d5e-9d57-0e3c5b6d7a11",
        "name": "orders",
        "description": description,
        "format": {"provider": "parquet", "options": {}},
        "schemaString": schema_string,
        "partitionColumns": partitions,
        "createdTime": 1_760_000_000_000_u64,
        "configuration": {
            "delta.columnMapping.mode": "name",
            "delta.columnMapping.maxColumnId": max_id.to_string(),
            "oml.owner": "data-platform",
            "oml.grain": "one row per order",
            "oml.freshness": "daily by 06:00 UTC",
            "delta.deletedFileRetentionDuration": "interval 1 week",
            "fs.s3a.secret.key": SENTINEL_SECRET
        }
    }})
}

fn add(n: u32) -> Value {
    add_with(n, "col-02b9")
}

fn add_with(n: u32, part_key: &str) -> Value {
    json!({"add": {
        "path": format!("region=EU/{SENTINEL_FILE}-{n}.parquet"),
        "partitionValues": if part_key.is_empty() { json!({}) } else { json!({part_key: SENTINEL_VALUE}) },
        "size": 1024 + n,
        "modificationTime": 1_760_000_000_000_u64 + u64::from(n),
        "dataChange": true,
        "stats": format!("{{\"numRecords\":42,\"minValues\":{{\"col-6f1a\":1}},\"maxValues\":{{\"col-6f1a\":9917,\"note\":\"{SENTINEL_STATS}\"}}}}")
    }})
}

fn info(op: &str) -> Value {
    json!({"commitInfo": {"timestamp": 1_760_000_000_000_u64, "operation": op,
        "userName": SENTINEL_USER, "notebook": {"notebookPath": "/Users/eugene/secret-notebook"}}})
}

fn commit(dir: &Path, version: u64, actions: &[Value]) {
    let name = format!("{version:020}.json");
    let body: Vec<String> = actions.iter().map(|a| a.to_string()).collect();
    fs::write(dir.join("_delta_log").join(name), body.join("\n") + "\n").unwrap();
}

fn orders(root: &Path) -> PathBuf {
    let dir = root.join("sales/orders");
    fs::create_dir_all(dir.join("_delta_log")).unwrap();
    let protocol = json!({"protocol": {"minReaderVersion": 3, "minWriterVersion": 7,
        "readerFeatures": ["columnMapping"], "writerFeatures": ["columnMapping"]}});
    let created = "Orders placed through the shop, one row per order";
    commit(&dir, 0, &[protocol, meta(orders_schema("amount", "Order identifier", false), &["region"], 6, Some(created)), add(0), info("CREATE TABLE")]);
    commit(&dir, 1, &[add(1), info("WRITE")]);
    commit(&dir, 2, &[meta(orders_schema("amount", "Order identifier", true), &["region"], 7, Some(created)), info("ADD COLUMNS")]);
    commit(&dir, 3, &[meta(orders_schema("total_amount", "Order identifier", true), &["region"], 7, Some(created)), info("RENAME COLUMN")]);
    commit(&dir, 4, &[add(4), info("WRITE")]);
    commit(&dir, 5, &[meta(orders_schema("total_amount", "Order identifier, unique per tenant", true), &["region"], 7, Some(created)), info("CHANGE COLUMN")]);
    dir
}

fn clicks(root: &Path) -> PathBuf {
    let dir = root.join("events/clicks");
    fs::create_dir_all(dir.join("_delta_log")).unwrap();
    let protocol = json!({"protocol": {"minReaderVersion": 1, "minWriterVersion": 7,
        "writerFeatures": ["domainMetadata", "clustering"]}});
    let sch = |desc: Option<&str>| {
        let s = schema(vec![
            field("ts", json!("timestamp"), true, None, 0, None),
            field("user_id", json!("string"), true, Some("Visitor identifier, pseudonymous"), 0, None),
            field("page", json!("string"), true, None, 0, None),
        ]);
        json!({"metaData": {
            "id": "a1c2e3f4-1111-4222-8333-944455556666", "name": "clicks", "description": desc,
            "format": {"provider": "parquet", "options": {}}, "schemaString": s,
            "partitionColumns": [], "createdTime": 1_760_000_000_000_u64,
            "configuration": {"oml.owner": "web-analytics", "delta.logRetentionDuration": "interval 30 days"}}})
    };
    let clustering = json!({"domainMetadata": {"domain": "delta.clustering", "removed": false,
        "configuration": "{\"clusteringColumns\":[[\"ts\"],[\"page\"]],\"domainName\":\"delta.clustering\"}"}});
    commit(&dir, 0, &[protocol, sch(None), clustering, add_with(10, ""), info("CREATE TABLE")]);
    commit(&dir, 1, &[add_with(11, ""), info("WRITE")]);
    commit(&dir, 2, &[add_with(12, ""), info("WRITE")]);
    commit(&dir, 3, &[add_with(13, ""), info("WRITE")]);
    commit(&dir, 4, &[sch(Some("Page views and clicks of the public site")), info("SET TBLPROPERTIES")]);
    dir
}

async fn load(dir: &Path, version: Option<u64>) -> Result<deltalake::DeltaTable, String> {
    let url = Url::from_directory_path(dir).unwrap();
    match version {
        Some(v) => deltalake::open_table_with_version(url, v).await.map_err(|e| e.to_string()),
        None => deltalake::open_table(url).await.map_err(|e| e.to_string()),
    }
}

async fn checkpoint(dir: &Path, at: u64) {
    let t = load(dir, Some(at)).await.expect("table loads for its checkpoint");
    deltalake::protocol::checkpoints::create_checkpoint(&t, None).await.expect("checkpoint");
}

async fn oracle(dir: &Path, name: &str, versions: &[u64]) -> Vec<Value> {
    let mut out = Vec::new();
    for &v in versions {
        let t = load(dir, Some(v)).await.unwrap_or_else(|e| panic!("{name}@{v} loads: {e:?}"));
        let s = t.snapshot().unwrap();
        let m = s.metadata();
        let schema = serde_json::to_value(s.schema().as_ref()).unwrap();
        out.push(json!({
            "table": name, "version": v,
            "name": m.name(), "description": m.description(),
            "partition_columns": m.partition_columns(),
            "column_mapping_mode": m.configuration().get("delta.columnMapping.mode"),
            "schema": schema,
        }));
    }
    out
}

#[tokio::main]
async fn main() {
    let root = PathBuf::from(std::env::args().nth(1).expect("the fixture directory"));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    let root = root.canonicalize().unwrap();
    let o = orders(&root);
    let c = clicks(&root);
    let mut records = Vec::new();
    records.extend(oracle(&o, "sales/orders", &[0, 1, 2, 3, 4, 5]).await);
    records.extend(oracle(&c, "events/clicks", &[0, 1, 2, 3, 4]).await);
    checkpoint(&o, 3).await;
    checkpoint(&c, 2).await;
    // The log before events/clicks' checkpoint is gone, as a retention
    // window leaves it: version 2 is readable from the checkpoint alone.
    for v in 0..2 {
        fs::remove_file(c.join("_delta_log").join(format!("{v:020}.json"))).unwrap();
    }
    // The oracle reads the pruned log, so what it says is what is there.
    let mut kept: Vec<Value> = records
        .into_iter()
        .filter(|r| !(r["table"] == "events/clicks" && r["version"].as_u64().unwrap() < 2))
        .collect();
    kept.extend(Vec::new());
    fs::write(
        root.join("oracle.json"),
        serde_json::to_string_pretty(&Value::Array(kept)).unwrap() + "\n",
    )
    .unwrap();
}
