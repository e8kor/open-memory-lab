use std::collections::{HashSet, VecDeque};
use std::process::Command;

use serde::Deserialize;

#[derive(Deserialize)]
struct Metadata {
    resolve: Resolve,
}

#[derive(Deserialize)]
struct Resolve {
    root: String,
    nodes: Vec<Node>,
}

#[derive(Deserialize)]
struct Node {
    id: String,
    deps: Vec<Edge>,
}

#[derive(Deserialize)]
struct Edge {
    pkg: String,
    dep_kinds: Vec<DepKind>,
}

#[derive(Deserialize)]
struct DepKind {
    kind: Option<String>,
}

const FORBIDDEN: &[&str] = &[
    "oml-app",
    "oml-domain",
    "oml-guardrail",
    "oml-scan",
    "oml-pg",
    "oml-rerank",
    "oml-transport-http",
    "oml-server-lead",
    "sqlx",
];

fn package_name(id: &str) -> String {
    id.split_whitespace().next().unwrap_or(id).to_owned()
}

#[test]
fn oml_web_reaches_no_application_crate() {
    let manifest = concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml");
    let output = Command::new("cargo")
        .args([
            "metadata",
            "--format-version",
            "1",
            "--locked",
            "--manifest-path",
            manifest,
        ])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("cargo metadata runs");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let metadata: Metadata = serde_json::from_slice(&output.stdout).expect("metadata JSON");
    let root = metadata.resolve.root.clone();

    let mut seen = HashSet::new();
    let mut queue = VecDeque::from([root]);
    let mut reached = HashSet::new();
    while let Some(node) = queue.pop_front() {
        if !seen.insert(node.clone()) {
            continue;
        }
        if package_name(&node) != "oml-web"
            && FORBIDDEN.iter().any(|name| package_name(&node) == *name)
        {
            panic!("oml-web reaches a forbidden crate: {}", node);
        }
        let adjacent = metadata
            .resolve
            .nodes
            .iter()
            .find(|n| n.id == node)
            .map(|n| {
                n.deps
                    .iter()
                    .filter(|edge| edge.dep_kinds.iter().any(|k| k.kind.is_none()))
                    .map(|edge| edge.pkg.clone())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        for dep in adjacent {
            reached.insert(package_name(&dep));
            queue.push_back(dep);
        }
    }
    // The crawl above enforces the application/domain boundary.
}
