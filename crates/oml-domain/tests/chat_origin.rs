//! The chat anchor's grammar and `origin.kind` (ADR-0063 d1-d5): one case
//! per rule, each refusal naming its field.

use oml_domain::anchor::{Anchor, turn_span};
use oml_domain::origin::{OriginKind, parse_kinds};
use serde_json::{Value, json};

const SESSION: &str = "0123456789abcdef0123456789abcdef";
const HASH: &str = "sha256:1f3a4f1a7b3d5e8c0f2a4b6d8e0c2f4a6b8d0e2c4f6a8b0d2e4c6f8a0b2d4e6c";

fn chat() -> Value {
    json!({
        "uri": format!("chat://claude-code/{SESSION}"),
        "fragment": "turn=7",
        "hash": HASH,
        "hasher": "oml-turn/1",
    })
}

fn with(field: &str, value: Value) -> Value {
    let mut anchor = chat();
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
fn a_chat_anchor_round_trips_as_declared_and_reads_as_chat() {
    let anchor = Anchor::from_json(&chat(), "/anchor").expect("well formed");
    assert!(anchor.is_chat());
    assert_eq!(anchor.to_json(), chat());
    let span = with("fragment", json!("turn=3-9"));
    assert_eq!(
        Anchor::from_json(&span, "/anchor")
            .expect("a span")
            .to_json(),
        span
    );
}

#[test]
fn a_malformed_chat_anchor_is_refused_naming_its_field() {
    let cases: Vec<(Value, &str)> = vec![
        // A raw harness session id, a UUID, is not the pseudonym (d5).
        (
            with(
                "uri",
                json!("chat://claude-code/00893aaf-19fa-41d2-8238-13269b9b3ca0"),
            ),
            "/anchor/uri",
        ),
        (
            with("uri", json!(format!("chat://Claude/{SESSION}"))),
            "/anchor/uri",
        ),
        (
            with("uri", json!(format!("chat://1code/{SESSION}"))),
            "/anchor/uri",
        ),
        (
            with("uri", json!(format!("chat://u@claude-code/{SESSION}"))),
            "/anchor/uri",
        ),
        (
            with("uri", json!(format!("chat://claude-code/{SESSION}/x"))),
            "/anchor/uri",
        ),
        (with("uri", json!("chat://claude-code/")), "/anchor/uri"),
        (
            with("uri", json!(format!("chat://{}/{SESSION}", "a".repeat(33)))),
            "/anchor/uri",
        ),
        (with("rev", json!("v1")), "/anchor/rev"),
        (with("path", json!("a.md")), "/anchor/path"),
        (with("fragment", Value::Null), "/anchor/fragment"),
        (with("fragment", json!("d1")), "/anchor/fragment"),
        (with("fragment", json!("turn=0")), "/anchor/fragment"),
        (with("fragment", json!("turn=07")), "/anchor/fragment"),
        (with("fragment", json!("turn=9-3")), "/anchor/fragment"),
        (with("fragment", json!("turn=100001")), "/anchor/fragment"),
        (with("hash", Value::Null), "/anchor/hash"),
        (
            json!({"uri": format!("chat://claude-code/{SESSION}"), "fragment": "turn=1"}),
            "/anchor/hash",
        ),
        (with("hasher", json!("oml-md/1")), "/anchor/hasher"),
    ];
    for (anchor, field) in cases {
        assert_eq!(refused(&anchor), field, "{anchor}");
    }
}

#[test]
fn turn_fragments_and_the_turn_hasher_belong_to_chat_anchors_alone() {
    let page = json!({"uri": "https://example.org/p", "fragment": "turn=1"});
    assert_eq!(refused(&page), "/anchor/fragment");
    let page = json!({"uri": "https://example.org/p", "hash": HASH, "hasher": "oml-turn/1"});
    assert_eq!(refused(&page), "/anchor/hasher");
}

#[test]
fn turn_spans_parse_within_bounds() {
    assert_eq!(turn_span("turn=1"), Some((1, 1)));
    assert_eq!(turn_span("turn=2-100000"), Some((2, 100_000)));
    for bad in ["turn=", "turn=-1", "turn=1-", "turn=a", "L1", "turn=1-2-3"] {
        assert_eq!(turn_span(bad), None, "{bad}");
    }
}

#[test]
fn origin_is_a_closed_object_whose_kind_is_one_of_seven() {
    for kind in ["repo", "doc", "code", "lake", "chat", "web", "manual"] {
        let parsed = OriginKind::from_json(&json!({"kind": kind}), "/origin").expect(kind);
        assert_eq!(parsed.to_json(), json!({"kind": kind}));
    }
    let field = |v: Value| {
        OriginKind::from_json(&v, "/origin")
            .expect_err("refused")
            .field
    };
    assert_eq!(field(json!({"kind": "email"})), "/origin/kind");
    assert_eq!(field(json!({})), "/origin/kind");
    assert_eq!(field(json!({"kind": "doc", "uri": "x"})), "/origin/uri");
    assert_eq!(field(json!("doc")), "/origin");
}

#[test]
fn chat_kind_and_a_chat_anchor_require_each_other() {
    let anchor = Anchor::from_json(&chat(), "/anchor").expect("chat");
    let page =
        Anchor::from_json(&json!({"uri": "https://example.org/p"}), "/anchor").expect("page");
    assert!(OriginKind::Chat.check_with(Some(&anchor)).is_ok());
    assert_eq!(
        OriginKind::Chat
            .check_with(Some(&page))
            .expect_err("no chat anchor")
            .field,
        "/origin/kind"
    );
    assert_eq!(
        OriginKind::Chat
            .check_with(None)
            .expect_err("unanchored")
            .field,
        "/origin/kind"
    );
    assert_eq!(
        OriginKind::Manual
            .check_with(Some(&anchor))
            .expect_err("chat anchor")
            .field,
        "/origin/kind"
    );
    assert!(OriginKind::Web.check_with(Some(&page)).is_ok());
    assert!(OriginKind::Manual.check_with(None).is_ok());
}

#[test]
fn the_kind_filter_takes_one_to_seven_distinct_kinds() {
    assert_eq!(
        parse_kinds(["chat", "doc"]).expect("two"),
        vec![OriginKind::Chat, OriginKind::Doc]
    );
    assert!(parse_kinds(Vec::<&str>::new()).is_err());
    assert!(parse_kinds(["chat", "chat"]).is_err());
    assert!(parse_kinds(["email"]).is_err());
}
