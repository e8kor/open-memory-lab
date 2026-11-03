//! The vocabulary release vectors of `docs/engineering/release-vectors.md`
//! reproduce byte for byte from `build_release` and `diff_releases`
//! (ADR-0021 d4). The test reads the page itself, so it keeps no copy of any
//! vector; `scripts/check_release_vectors.py` rebuilds the same vectors
//! outside Rust.

use std::collections::BTreeMap;

use oml_domain::canonical_bytes;
use oml_domain::vocab::{Curie, OmlLabel, Successors};
use oml_domain::vocab_edges::{TermEdge, TermTip, TipState};
use oml_domain::vocab_release::{
    ReleaseClosureRow, ReleaseDiff, ReleaseSnapshot, build_release, diff_releases,
};
use serde_json::{Value, json};

const PAGE: &str = include_str!("../../../docs/engineering/release-vectors.md");

/// One vector's three blocks, as the page's `<!-- vector: ID BLOCK -->`
/// comments name the fenced block after each.
#[derive(Default)]
struct Vector {
    input: Option<String>,
    canonical: Option<String>,
    digest: Option<String>,
}

fn vectors() -> BTreeMap<String, Vector> {
    let mut vectors: BTreeMap<String, Vector> = BTreeMap::new();
    for chunk in PAGE.split("<!-- vector: ").skip(1) {
        let (name, rest) = chunk.split_once(" -->").expect("a closed vector comment");
        let (id, block) = name
            .split_once(' ')
            .expect("`ID BLOCK` in a vector comment");
        let fence = rest.trim_start();
        assert!(
            fence.starts_with("```"),
            "{id} {block}: a fenced block follows"
        );
        let body = &fence[fence.find('\n').expect("a fence line") + 1..];
        let body = &body[..body.find("\n```").expect("a closing fence")];
        let vector = vectors.entry(id.to_owned()).or_default();
        let slot = match block {
            "input" => &mut vector.input,
            "canonical" => &mut vector.canonical,
            "digest" => &mut vector.digest,
            other => panic!("{id}: unknown block {other}"),
        };
        assert!(slot.is_none(), "{id} has two {block} blocks");
        *slot = Some(body.to_owned());
    }
    vectors
}

fn curie(value: &Value) -> Curie {
    Curie::parse(value.as_str().expect("a CURIE string")).expect("a well-formed CURIE")
}

fn tip(value: &Value) -> TermTip {
    let state = match value["term_status"].as_str() {
        Some("active") => TipState::Active,
        Some("deprecated") => {
            let curies: Vec<Curie> = value["successors"]["curies"]
                .as_array()
                .expect("successor curies")
                .iter()
                .map(curie)
                .collect();
            TipState::Deprecated(match value["successors"]["relation"].as_str() {
                Some("replaced_by") => {
                    let [one] = <[Curie; 1]>::try_from(curies).expect("one replaced_by successor");
                    Successors::ReplacedBy(one)
                }
                Some("consider") => Successors::Consider(curies),
                other => panic!("unknown relation {other:?}"),
            })
        }
        other => panic!("unknown term_status {other:?}"),
    };
    TermTip {
        curie: curie(&value["curie"]),
        state,
    }
}

fn edge(value: &Value) -> TermEdge {
    TermEdge {
        label: OmlLabel::parse(value["label"].as_str().expect("a label")).expect("an oml: label"),
        from: curie(&value["from"]),
        to: curie(&value["to"]),
    }
}

fn release(input: &Value) -> ReleaseSnapshot {
    let list = |key: &str| input[key].as_array().expect("an array").clone();
    let tips: Vec<TermTip> = list("tips").iter().map(tip).collect();
    let edges: Vec<TermEdge> = list("edges").iter().map(edge).collect();
    build_release(&tips, &edges).expect("a vector's release builds")
}

fn term(curie: &Curie, state: &TipState) -> Value {
    json!({
        "curie": curie.as_str(),
        "successors": successors(state),
        "term_status": state.status().as_str(),
    })
}

fn successors(state: &TipState) -> Vec<Value> {
    match state {
        TipState::Active => Vec::new(),
        TipState::Deprecated(s) => s
            .curies()
            .iter()
            .map(|c| json!({"curie": c.as_str(), "relation": s.relation().as_str()}))
            .collect(),
    }
}

fn edges<'a>(edges: impl IntoIterator<Item = &'a TermEdge>) -> Vec<Value> {
    edges
        .into_iter()
        .map(|e| json!({"from": e.from.as_str(), "label": e.label.as_str(), "to": e.to.as_str()}))
        .collect()
}

fn rows<'a>(rows: impl IntoIterator<Item = &'a ReleaseClosureRow>) -> Vec<Value> {
    rows.into_iter()
        .map(|r| {
            json!({
                "ancestor": r.ancestor.as_str(),
                "depth": r.depth,
                "descendant": r.descendant.as_str(),
            })
        })
        .collect()
}

/// The page's own shape for a diff, which no served format declares
/// (release-vectors.md, "The diff").
fn diff_document(diff: &ReleaseDiff) -> Vec<u8> {
    let deprecations: Vec<Value> = diff
        .deprecations
        .iter()
        .map(|(c, s)| {
            json!({
                "curie": c.as_str(),
                "successors": successors(&TipState::Deprecated(s.clone())),
            })
        })
        .collect();
    let terms_added: Vec<Value> = diff.terms_added.iter().map(|(c, s)| term(c, s)).collect();
    canonical_bytes(&json!({
        "closure_added": rows(&diff.closure_added),
        "closure_removed": rows(&diff.closure_removed),
        "deprecations": deprecations,
        "edges_added": edges(&diff.edges_added),
        "edges_removed": edges(&diff.edges_removed),
        "terms_added": terms_added,
    }))
    .expect("strings and u32 depths canonicalize")
}

/// The page's `bytes  = N` and `sha256 = sha256:<hex>`.
fn stated_digest(block: &str) -> (usize, String) {
    let mut lines = block.lines();
    let count = lines
        .next()
        .and_then(|l| l.strip_prefix("bytes  = "))
        .expect("a `bytes  = N` line")
        .parse()
        .expect("a byte count");
    let digest = lines
        .next()
        .and_then(|l| l.strip_prefix("sha256 = "))
        .expect("a `sha256 = ` line")
        .to_owned();
    assert!(lines.next().is_none(), "two lines in a digest block");
    (count, digest)
}

fn assert_vector(id: &str, vector: &Vector, bytes: &[u8]) {
    let canonical = vector.canonical.as_deref().expect("a canonical block");
    let (count, digest) = stated_digest(vector.digest.as_deref().expect("a digest block"));
    assert_eq!(
        String::from_utf8_lossy(bytes),
        canonical,
        "{id}: the canonical bytes"
    );
    assert_eq!(bytes.len(), count, "{id}: the stated byte count");
    assert_eq!(
        oml_domain::Sha256Hash::digest(bytes).to_string(),
        digest,
        "{id}: the stated digest"
    );
}

#[test]
fn every_release_and_diff_vector_on_the_page_reproduces_byte_for_byte() {
    let vectors = vectors();
    let mut releases: BTreeMap<&str, ReleaseSnapshot> = BTreeMap::new();
    for (id, vector) in vectors.iter().filter(|(id, _)| id.starts_with('R')) {
        let input: Value =
            serde_json::from_str(vector.input.as_deref().expect("an input")).expect("a JSON input");
        let snapshot = release(&input);
        assert_vector(id, vector, snapshot.canonical_bytes());
        assert_eq!(
            snapshot.digest(),
            oml_domain::Sha256Hash::digest(snapshot.canonical_bytes()),
            "{id}: the snapshot's digest is SHA-256 of its bytes"
        );
        releases.insert(id, snapshot);
    }
    let diffs: Vec<_> = vectors
        .iter()
        .filter(|(id, _)| id.starts_with('D'))
        .collect();
    for (id, vector) in &diffs {
        let input: Value =
            serde_json::from_str(vector.input.as_deref().expect("an input")).expect("a JSON input");
        let named = |key: &str| &releases[input[key].as_str().expect("a release vector id")];
        let diff = diff_releases(named("previous"), named("next")).expect("a vector's diff");
        assert_vector(id, vector, &diff_document(&diff));
    }
    assert_eq!(
        (releases.len(), diffs.len()),
        (3, 2),
        "the page's three release vectors and two diff vectors are all read"
    );
}

#[test]
fn a_release_vectors_bytes_do_not_depend_on_the_order_of_its_input() {
    for (id, vector) in vectors().iter().filter(|(id, _)| id.starts_with('R')) {
        let mut input: Value =
            serde_json::from_str(vector.input.as_deref().expect("an input")).expect("JSON");
        let forward = release(&input);
        for key in ["tips", "edges"] {
            input[key].as_array_mut().expect("an array").reverse();
        }
        assert_eq!(
            release(&input).canonical_bytes(),
            forward.canonical_bytes(),
            "{id}"
        );
    }
}
