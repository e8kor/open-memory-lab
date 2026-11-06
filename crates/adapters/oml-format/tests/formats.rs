//! The adapters of ADR-0056, run through the compiled registry: what each
//! accepts, refuses by name, derives for search and compares.

use oml_app::ports::{Format, Formats};
use oml_format::CompiledFormats;

const MERMAID: &str = "text/vnd.oml.mermaid";

fn adapter(media: &str) -> std::sync::Arc<dyn Format> {
    CompiledFormats
        .resolve(media)
        .unwrap_or_else(|allowed| panic!("{media} is refused; accepted: {allowed:?}"))
        .adapter
}

fn search_text(media: &str, source: &str) -> String {
    let a = adapter(media);
    let parsed = a.validate(source).unwrap_or_else(|e| panic!("{e}"));
    a.search_text(&parsed)
}

fn refusal(media: &str, source: &str) -> (&'static str, u32) {
    match adapter(media).validate(source) {
        Ok(_) => panic!("{source:?} was accepted"),
        Err(e) => (e.reason, e.line),
    }
}

#[test]
fn the_closed_list_names_every_media_type_and_a_refusal_lists_them() {
    let allowed = CompiledFormats.allowed();
    for want in [
        "text/plain",
        "text/markdown",
        "text/vnd.oml.mermaid",
        "application/json",
        "application/yaml",
        "text/vnd.oml.code; lang=rust",
    ] {
        assert!(
            allowed.iter().any(|a| a == want),
            "{want} is not allowed: {allowed:?}"
        );
    }
    assert!(
        CompiledFormats.resolve("mermaid").is_err(),
        "no short alias"
    );
    assert!(
        CompiledFormats
            .resolve("text/vnd.oml.code; lang=cobol")
            .is_err()
    );
    assert_eq!(
        CompiledFormats
            .resolve("text/vnd.oml.code;lang=rust")
            .expect("rust")
            .media_type,
        "text/vnd.oml.code; lang=rust",
        "one canonical spelling is signed"
    );
}

#[test]
fn a_mermaid_relation_becomes_one_sentence_in_source_order() {
    let source =
        "flowchart LR\n  A[Checkout] -->|calls| B[Payments]\n  B --> C[Ledger]\n  D[Orphan]\n";
    assert_eq!(
        search_text(MERMAID, source),
        "Checkout (A) calls Payments (B).\nPayments (B) leads to Ledger (C).\nOrphan (D)."
    );
    assert_eq!(
        search_text(MERMAID, "flowchart TD\n  Checkout --> Payments\n"),
        "Checkout leads to Payments."
    );
    assert_eq!(
        search_text(
            MERMAID,
            "sequenceDiagram\n  participant Checkout\n  participant Payments\n  Checkout->>Payments: authorize amount\n"
        ),
        "Checkout sends Payments: authorize amount."
    );
    assert_eq!(
        search_text(
            MERMAID,
            "classDiagram\n  PaymentMethod <|-- Card\n  Order *-- LineItem\n"
        ),
        "Card is a PaymentMethod.\nOrder has LineItem."
    );
    assert_eq!(
        search_text(MERMAID, "stateDiagram-v2\n  Pending --> Paid : capture\n"),
        "From Pending to Paid on capture."
    );
    assert_eq!(
        search_text(MERMAID, "erDiagram\n  Customer ||--o{ Order : places\n"),
        "Customer places Order, one to many."
    );
}

#[test]
fn a_mermaid_comment_never_reaches_the_search_text() {
    let text = search_text(
        MERMAID,
        "flowchart LR\n  %% the secret plan is hidden here\n  A --> B\n",
    );
    assert_eq!(text, "A leads to B.");
}

#[test]
fn a_mermaid_directive_click_or_style_is_refused_by_name() {
    let cases = [
        (
            "%%{init: {\"securityLevel\": \"loose\"}}%%\nflowchart LR\n A-->B",
            "mermaid.directive_refused",
        ),
        (
            "---\nconfig:\n  theme: dark\n---\nflowchart LR\n A-->B",
            "mermaid.directive_refused",
        ),
        (
            "flowchart LR\n A-->B\n click A callback \"tip\"",
            "mermaid.interaction_refused",
        ),
        (
            "flowchart LR\n A-->B\n click A href \"https://example.com\"",
            "mermaid.interaction_refused",
        ),
        (
            "flowchart LR\n A-->B\n style A fill:#fff",
            "mermaid.styling_refused",
        ),
        (
            "flowchart LR\n A-->B\n classDef hide display:none",
            "mermaid.styling_refused",
        ),
        (
            "flowchart LR\n A-->B\n class A hide",
            "mermaid.styling_refused",
        ),
        (
            "flowchart LR\n A-->B\n linkStyle 0 stroke:#fff",
            "mermaid.styling_refused",
        ),
        ("flowchart LR\n A:::hide --> B", "mermaid.styling_refused"),
        (
            "flowchart LR\n A[<img src=x onerror=alert(1)>] --> B",
            "mermaid.html_label_refused",
        ),
        ("flowchart LR\n A[x\u{202e}y] --> B", "format.bidi_control"),
    ];
    for (source, reason) in cases {
        assert_eq!(refusal(MERMAID, source).0, reason, "{source:?}");
    }
    assert_eq!(
        refusal(MERMAID, "flowchart LR\n A-->B\n style A fill:#fff").1,
        3,
        "the line it was found on"
    );
}

#[test]
fn an_unsupported_mermaid_diagram_is_refused_by_name() {
    for header in [
        "gantt\n title x",
        "pie\n \"a\": 1",
        "timeline\n title x",
        "journey\n title x",
        "mindmap\n root",
        "quadrantChart\n title x",
        "sankey-beta\n a,b,1",
        "xychart-beta\n title x",
        "gitGraph\n commit",
        "C4Context\n title x",
        "block-beta\n columns 1",
        "architecture-beta\n group a",
        "notADiagram\n a",
    ] {
        assert_eq!(
            refusal(MERMAID, header).0,
            "mermaid.unsupported_diagram",
            "{header}"
        );
    }
}

#[test]
fn a_bidirectional_control_is_refused_in_every_non_prose_format() {
    for (media, source) in [
        ("application/json", "{\"a\": \"x\u{202e}y\"}"),
        ("application/yaml", "a: \"x\u{2066}y\"\n"),
        ("text/vnd.oml.code; lang=rust", "let a = \"x\u{202a}y\";"),
    ] {
        assert_eq!(refusal(media, source).0, "format.bidi_control", "{media}");
    }
}

#[test]
fn a_json_duplicate_key_is_refused() {
    assert_eq!(
        refusal("application/json", "{\n \"a\": 1,\n \"a\": 2\n}").0,
        "json.duplicate_key"
    );
    assert_eq!(
        refusal("application/json", "{\"a\": ").0,
        "json.invalid",
        "a syntax error is named too"
    );
}

#[test]
fn a_yaml_alias_tag_or_second_document_is_refused() {
    for (source, reason) in [
        ("a: &x 1\nb: *x\n", "yaml.alias_refused"),
        ("a: !!str 1\n", "yaml.tag_refused"),
        ("a: 1\n---\nb: 2\n", "yaml.second_document"),
        ("a: 1\na: 2\n", "yaml.duplicate_key"),
        ("base: &b {x: 1}\nd:\n  <<: *b\n", "yaml.alias_refused"),
        ("a: [1\n", "yaml.invalid"),
    ] {
        assert_eq!(refusal("application/yaml", source).0, reason, "{source:?}");
    }
}

#[test]
fn json_and_yaml_search_text_is_one_line_per_scalar_with_its_key_path() {
    let want = "payments retry max_attempts is 5\npayments retry backoff is exponential\ntags 0 is a\ntags 1 is b";
    assert_eq!(
        search_text(
            "application/json",
            "{\"payments\":{\"retry\":{\"max_attempts\":5,\"backoff\":\"exponential\"}},\"tags\":[\"a\",\"b\"]}"
        ),
        want
    );
    assert_eq!(
        search_text(
            "application/yaml",
            "payments:\n    retry:\n        max_attempts: 5\n        backoff: exponential\ntags:\n  - a\n  - b\n"
        ),
        want
    );
}

#[test]
fn a_code_snippet_is_indexed_as_its_source() {
    let source = "fn retry_budget(max_attempts: u32) -> u32 { max_attempts * 2 }";
    assert_eq!(search_text("text/vnd.oml.code; lang=rust", source), source);
}

#[test]
fn the_scan_vetoes_a_private_key_hidden_by_a_json_escape() {
    let a = adapter("application/json");
    let source = "{\"note\": \"\\u002d\\u002d\\u002d-BEGIN PRIVATE KEY\"}";
    assert!(
        !source.contains("-----BEGIN"),
        "the source alone shows no key"
    );
    let parsed = a.validate(source).expect("valid json");
    let views = a.scan_views(&parsed);
    assert!(
        views
            .iter()
            .any(|v| v.path == "note" && v.text.contains("-----BEGIN PRIVATE KEY")),
        "{views:?}"
    );
}

#[test]
fn the_scan_vetoes_a_private_key_in_a_folded_yaml_scalar() {
    let a = adapter("application/yaml");
    let source = "note: >\n  -----BEGIN\n  PRIVATE KEY\n";
    let parsed = a.validate(source).expect("valid yaml");
    let views = a.scan_views(&parsed);
    assert!(
        views
            .iter()
            .any(|v| v.path == "note" && v.text.contains("-----BEGIN PRIVATE KEY")),
        "{views:?}"
    );
}

#[test]
fn a_value_under_a_secret_shaped_key_is_vetoed_naming_the_path_never_the_value() {
    for (media, source) in [
        (
            "application/json",
            "{\"db\": {\"password\": \"hunter2hunter2\"}}",
        ),
        ("application/yaml", "db:\n  api_key: hunter2hunter2\n"),
        (
            "application/json",
            "{\"stripe_secret\": \"hunter2hunter2\"}",
        ),
    ] {
        let a = adapter(media);
        let parsed = a.validate(source).expect("valid");
        let veto = parsed.veto.expect("vetoed");
        assert!(veto.starts_with("secret.keyed_value"), "{veto}");
        assert!(!veto.contains("hunter2"), "never the value: {veto}");
    }
    let a = adapter("application/json");
    let veto = a
        .validate("{\"db\": {\"password\": \"hunter2hunter2\"}}")
        .expect("valid")
        .veto
        .expect("vetoed");
    assert!(veto.contains("db.password"), "the path: {veto}");
}

#[test]
fn a_placeholder_under_a_secret_shaped_key_passes() {
    for value in [
        "",
        "<your token>",
        "${TOKEN}",
        "{{ token }}",
        "***",
        "changeme",
        "example",
        "redacted",
    ] {
        let source = format!("{{\"token\": \"{value}\"}}");
        let parsed = adapter("application/json")
            .validate(&source)
            .expect("valid");
        assert_eq!(parsed.veto, None, "{value:?}");
    }
}

#[test]
fn no_secret_shaped_value_reaches_the_search_text_or_the_postings() {
    let text = search_text(
        "application/json",
        "{\"token\": \"changeme\", \"host\": \"db1\"}",
    );
    assert_eq!(text, "host is db1");
}

#[test]
fn reordered_json_keys_and_reindented_yaml_are_equivalent() {
    let j = adapter("application/json");
    let d = j
        .diff(
            "{\"a\":1,\"b\":{\"c\":2}}",
            "{\n \"b\": {\"c\": 2},\n \"a\": 1\n}",
        )
        .expect("diff");
    assert!(d.equivalent && d.changes.is_empty(), "{d:?}");
    let y = adapter("application/yaml");
    let d = y.diff("a:\n  b: 1\n", "a:\n      b: 1\n").expect("diff");
    assert!(d.equivalent, "{d:?}");
    let d = y.diff("a:\n  b: 1\n", "a:\n  b: 2\n").expect("diff");
    assert!(!d.equivalent);
    assert_eq!(d.changes.len(), 1);
    assert_eq!(d.changes[0].path, "a.b");
}

#[test]
fn an_added_mermaid_edge_is_one_change_naming_the_edge() {
    let m = adapter(MERMAID);
    let d = m
        .diff(
            "flowchart LR\n A --> B\n",
            "flowchart LR\n  A --> B\n  B --> C\n",
        )
        .expect("diff");
    assert!(!d.equivalent);
    assert_eq!(d.changes.len(), 1, "{d:?}");
    assert_eq!(d.changes[0].op, "add");
    assert_eq!(d.changes[0].after.as_deref(), Some("B leads to C."));
    let same = m
        .diff("flowchart LR\n A --> B\n", "flowchart LR\n\n    A --> B\n")
        .expect("diff");
    assert!(same.equivalent, "a reflow is equivalent");
}

#[test]
fn a_code_diff_compares_lines_after_span_normalisation() {
    let c = adapter("text/vnd.oml.code; lang=rust");
    assert!(
        c.diff("fn a() {\n    1\n}\n", "fn a() {\n\t1\n}\n\n")
            .expect("diff")
            .equivalent
    );
    assert!(
        !c.diff("fn a() { 1 }", "fn a() { 2 }")
            .expect("diff")
            .equivalent
    );
}

#[test]
fn search_text_is_the_same_on_every_run() {
    let source = "flowchart LR\n  A[Checkout] -->|calls| B[Payments]\n";
    let first = search_text(MERMAID, source);
    for _ in 0..20 {
        assert_eq!(search_text(MERMAID, source), first);
    }
}

#[test]
fn the_caps_hold() {
    let big = format!("{{\"a\": \"{}\"}}", "x".repeat(70 * 1024));
    assert_eq!(refusal("application/json", &big).0, "format.too_large");
    let deep = format!("{}1{}", "[".repeat(40), "]".repeat(40));
    assert_eq!(refusal("application/json", &deep).0, "format.too_deep");
    let many: String = (0..201).map(|i| format!("  N{i} --> M{i}\n")).collect();
    assert_eq!(
        refusal(MERMAID, &format!("flowchart LR\n{many}")).0,
        "mermaid.too_many_nodes"
    );
}
