//! The anchor's grammar (ADR-0057 d3, d21): one case per row of d3, each
//! refusal naming its field, and the fragment forms a chunk may name.

use oml_domain::anchor::{Anchor, CheckResult, SourceCheckBody, line_span, secret_shaped};
use serde_json::{Value, json};

const REV: &str = "ca61c9edd25895702e4ddc89df1848aae696d9ba";
const HASH: &str = "sha256:1f3a4f1a7b3d5e8c0f2a4b6d8e0c2f4a6b8d0e2c4f6a8b0d2e4c6f8a0b2d4e6c";

fn repo_anchor() -> Value {
    json!({
        "repo": "github.com/e8kor/open-memory-lab",
        "rev": REV,
        "path": "docs/adr/ADR-0001-the-constitution.md",
        "fragment": "d1",
        "hash": HASH,
        "hasher": "oml-md/1",
    })
}

fn with(field: &str, value: Value) -> Value {
    let mut anchor = repo_anchor();
    if value.is_null() {
        anchor.as_object_mut().expect("object").remove(field);
    } else {
        anchor[field] = value;
    }
    anchor
}

fn refused(anchor: &Value) -> String {
    Anchor::from_json(anchor, "/anchor")
        .expect_err("refused")
        .field
}

#[test]
fn a_well_formed_anchor_round_trips_as_declared() {
    let anchor = Anchor::from_json(&repo_anchor(), "/anchor").expect("well formed");
    assert_eq!(anchor.to_json(), repo_anchor());
    let page = json!({"uri": "https://example.org/wiki/Page", "rev": "W/\"etag-7\""});
    assert_eq!(
        Anchor::from_json(&page, "/anchor")
            .expect("a page")
            .to_json(),
        page
    );
}

#[test]
fn a_malformed_anchor_is_request_validation_naming_its_field() {
    let cases: Vec<(Value, &str)> = vec![
        // repo
        (with("repo", json!("GitHub.com/x")), "/anchor/repo"),
        (with("repo", json!("git@github.com:x/y")), "/anchor/repo"),
        (with("repo", json!("a/b/c/d/e/f/g/h/i")), "/anchor/repo"),
        (with("repo", json!("a/../b")), "/anchor/repo"),
        (with("repo", json!("x".repeat(201))), "/anchor/repo"),
        (with("repo", Value::Null), "/anchor/repo"),
        // uri beside repo, and a bad uri
        (with("uri", json!("https://example.org")), "/anchor/uri"),
        (json!({"uri": "http://example.org/page"}), "/anchor/uri"),
        (
            json!({"uri": "https://user:pw@example.org/page"}),
            "/anchor/uri",
        ),
        (
            json!({"uri": "https://example.org/page#part"}),
            "/anchor/uri",
        ),
        // rev
        (with("rev", Value::Null), "/anchor/rev"),
        (with("rev", json!("HEAD")), "/anchor/rev"),
        (with("rev", json!(REV.to_uppercase())), "/anchor/rev"),
        (
            json!({"uri": "https://example.org/p", "rev": "has space"}),
            "/anchor/rev",
        ),
        // path
        (with("path", Value::Null), "/anchor/path"),
        (with("path", json!("/etc/passwd")), "/anchor/path"),
        (with("path", json!("docs/../secret")), "/anchor/path"),
        (with("path", json!("docs//adr")), "/anchor/path"),
        (with("path", json!("docs\\adr")), "/anchor/path"),
        (with("path", json!("e\u{301}.md")), "/anchor/path"),
        (with("path", json!("a".repeat(1025))), "/anchor/path"),
        (
            json!({"uri": "https://example.org/p", "path": "x.md"}),
            "/anchor/path",
        ),
        // fragment
        (with("fragment", json!("")), "/anchor/fragment"),
        (with("fragment", json!("two words")), "/anchor/fragment"),
        (with("fragment", json!("a#b")), "/anchor/fragment"),
        (with("fragment", json!("x".repeat(257))), "/anchor/fragment"),
        // hash and hasher
        (with("hasher", Value::Null), "/anchor/hasher"),
        (with("hash", Value::Null), "/anchor/hash"),
        (with("hash", json!("sha256:XYZ")), "/anchor/hash"),
        (with("hasher", json!("oml-span/1")), "/anchor/hasher"),
        // an unknown member
        (with("branch", json!("main")), "/anchor/branch"),
    ];
    for (anchor, field) in cases {
        assert_eq!(refused(&anchor), field, "{anchor}");
    }
    assert_eq!(
        Anchor::from_json(&json!("docs/adr"), "/anchor")
            .expect_err("not an object")
            .field,
        "/anchor"
    );
}

#[test]
fn a_malformed_line_span_or_a_page_fragment_is_request_validation() {
    for good in [
        "L1",
        "L10-L42",
        "L7-L7",
        "d3",
        "the-constitution",
        "L-not-a-span",
    ] {
        assert!(
            Anchor::from_json(&with("fragment", json!(good)), "/anchor").is_ok(),
            "{good}"
        );
    }
    for bad in [
        "L0",
        "L01",
        "L9-L3",
        "L10-",
        "L10-L",
        "L1x",
        "L10000001",
        "page=3",
    ] {
        assert_eq!(
            refused(&with("fragment", json!(bad))),
            "/anchor/fragment",
            "{bad}"
        );
    }
    assert_eq!(line_span("L10-L42"), Some((10, 42)));
    assert_eq!(line_span("L5"), Some((5, 5)));
    assert_eq!(line_span("L5-L4"), None);
}

#[test]
fn a_secret_shaped_path_is_recognised_by_its_file_name() {
    for path in [
        ".env",
        "config/.env.local",
        "certs/server.pem",
        "deploy/tls.key",
        "home/id_ed25519",
        "aws/credentials",
        "app/client_secret.json",
    ] {
        assert!(secret_shaped(path), "{path}");
    }
    for path in ["docs/adr/ADR-0001.md", "src/keys.rs", "environment.md"] {
        assert!(!secret_shaped(path), "{path}");
    }
}

#[test]
fn a_source_check_is_checked_against_its_anchor() {
    let anchor = Anchor::from_json(&repo_anchor(), "/anchor").expect("anchor");
    let moved = SourceCheckBody::check(
        &anchor,
        REV,
        CheckResult::Moved,
        Some(HASH),
        Some("docs/adr/ADR-0001-renamed.md"),
        Some("d1"),
    )
    .expect("a move");
    assert_eq!(
        moved.moved_to,
        Some((
            "docs/adr/ADR-0001-renamed.md".to_owned(),
            Some("d1".to_owned())
        ))
    );
    let field = |r: CheckResult, rev: &str, path: Option<&str>| {
        SourceCheckBody::check(&anchor, rev, r, None, path, None)
            .expect_err("refused")
            .field
    };
    assert_eq!(field(CheckResult::Fresh, "HEAD", None), "/rev");
    assert_eq!(field(CheckResult::Moved, REV, None), "/path");
    assert_eq!(field(CheckResult::Changed, REV, Some("x.md")), "/path");
    let unhashed = Anchor::from_json(&with("hash", Value::Null), "/anchor");
    assert!(unhashed.is_err());
    assert_eq!(CheckResult::Moved.stale(), None);
    assert_eq!(CheckResult::Equivalent.stale(), None);
    assert_eq!(CheckResult::Changed.stale(), Some("changed"));
    assert_eq!(CheckResult::Removed.stale(), Some("removed"));
}

fn lake(uri: &str, fragment: Option<&str>, hasher: &str) -> Value {
    let mut anchor = json!({ "uri": uri, "rev": "5", "hash": HASH, "hasher": hasher });
    if let Some(f) = fragment {
        anchor["fragment"] = json!(f);
    }
    anchor
}

#[test]
fn a_lake_anchor_carries_a_location_a_delta_version_a_column_fragment_and_a_lake_hasher() {
    for (uri, fragment, hasher) in [
        (
            "s3://acme-lake/warehouse/sales/orders",
            None,
            "oml-delta-table/1",
        ),
        ("gs://acme-lake/sales/orders", None, "oml-delta-schema/1"),
        (
            "https://acct.dfs.core.windows.net/raw/sales/orders",
            Some("col=customer.email"),
            "oml-delta-col/1",
        ),
        (
            "file:///srv/lake/sales/orders",
            Some("col=id"),
            "oml-delta-col/1",
        ),
    ] {
        let anchor = lake(uri, fragment, hasher);
        assert_eq!(
            Anchor::from_json(&anchor, "/anchor")
                .unwrap_or_else(|e| panic!("{anchor}: {e}"))
                .to_json(),
            anchor
        );
    }
}

#[test]
fn a_lake_anchor_with_user_information_a_query_a_fragment_or_a_bad_bucket_is_refused() {
    for uri in [
        "s3://key:secret@acme-lake/orders",
        "gs://acme-lake/orders?versionId=1",
        "s3://acme-lake/orders#frag",
        "s3://Acme_Lake/orders",
        "s3://ab/orders",
        "s3://acme-lake/orders/",
        "s3:///orders",
        "file://host/orders",
        "file:///orders/",
        "ftp://acme-lake/orders",
        "s3://acme-lake/Orders\u{0301}",
    ] {
        assert_eq!(
            refused(&lake(uri, None, "oml-delta-table/1")),
            "/anchor/uri",
            "{uri}"
        );
    }
}
