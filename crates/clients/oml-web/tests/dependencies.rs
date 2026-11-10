//! The wall of ADR-0070 d1: `oml-web`'s normal and build dependencies reach
//! no application crate, and of the workspace's own crates only `oml-page`.
//! Dev-dependencies are outside the walk.

use std::collections::{BTreeSet, HashMap, VecDeque};
use std::process::Command;

use serde::Deserialize;

#[derive(Deserialize)]
struct Metadata {
    packages: Vec<Package>,
    workspace_members: Vec<String>,
    resolve: Resolve,
}

#[derive(Deserialize)]
struct Package {
    id: String,
    name: String,
}

#[derive(Deserialize)]
struct Resolve {
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

/// Named by ADR-0070 d1: the application core, its adapters, the server.
const FORBIDDEN: &[&str] = &[
    "oml-app",
    "oml-domain",
    "oml-guardrail",
    "oml-scan",
    "oml-pg",
    "oml-lake",
    "oml-fetch",
    "oml-rerank",
    "oml-transport-http",
    "oml-server-lead",
    "sqlx",
];

/// The workspace crates `oml-web` may reach: the shared page crate, and the
/// Rust SDK once it exists (ADR-0070 d9).
const WORKSPACE_ALLOWED: &[&str] = &["oml-page", "oml-sdk"];

struct Graph {
    names: HashMap<String, String>,
    members: BTreeSet<String>,
    edges: HashMap<String, Vec<String>>,
}

fn graph() -> Graph {
    let output = Command::new(env!("CARGO"))
        .args(["metadata", "--format-version", "1", "--locked"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .expect("cargo metadata runs");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let metadata: Metadata = serde_json::from_slice(&output.stdout).expect("metadata JSON");
    let names = metadata
        .packages
        .into_iter()
        .map(|p| (p.id, p.name))
        .collect::<HashMap<_, _>>();
    let members = metadata
        .workspace_members
        .iter()
        .map(|id| names[id].clone())
        .collect();
    // Normal (`kind` null) and build edges; dev edges are outside the wall.
    let edges = metadata
        .resolve
        .nodes
        .into_iter()
        .map(|n| {
            let deps = n
                .deps
                .into_iter()
                .filter(|e| {
                    e.dep_kinds
                        .iter()
                        .any(|k| k.kind.as_deref().is_none_or(|k| k == "build"))
                })
                .map(|e| e.pkg)
                .collect();
            (n.id, deps)
        })
        .collect();
    Graph {
        names,
        members,
        edges,
    }
}

/// Every crate `oml-web` reaches, with the path that reaches it.
fn reached(g: &Graph) -> Vec<(String, Vec<String>)> {
    let root = g
        .names
        .iter()
        .find(|(_, name)| name.as_str() == "oml-web")
        .map(|(id, _)| id.clone())
        .expect("oml-web is in the graph");
    let mut paths: HashMap<String, Vec<String>> = HashMap::new();
    paths.insert(root.clone(), vec!["oml-web".to_owned()]);
    let mut queue = VecDeque::from([root]);
    while let Some(id) = queue.pop_front() {
        let path = paths[&id].clone();
        for dep in g.edges.get(&id).into_iter().flatten() {
            if !paths.contains_key(dep) {
                let mut next = path.clone();
                next.push(g.names[dep].clone());
                paths.insert(dep.clone(), next);
                queue.push_back(dep.clone());
            }
        }
    }
    paths
        .into_iter()
        .map(|(id, path)| (g.names[&id].clone(), path))
        .filter(|(name, _)| name != "oml-web")
        .collect()
}

#[test]
fn oml_web_reaches_no_application_crate() {
    let g = graph();
    let bad: Vec<String> = reached(&g)
        .into_iter()
        .filter(|(name, _)| FORBIDDEN.contains(&name.as_str()))
        .map(|(_, path)| path.join(" -> "))
        .collect();
    assert!(bad.is_empty(), "oml-web reaches forbidden crates: {bad:#?}");
}

#[test]
fn of_the_workspace_crates_oml_web_reaches_only_oml_page_and_the_sdk() {
    let g = graph();
    let bad: Vec<String> = reached(&g)
        .into_iter()
        .filter(|(name, _)| g.members.contains(name) && !WORKSPACE_ALLOWED.contains(&name.as_str()))
        .map(|(_, path)| path.join(" -> "))
        .collect();
    assert!(bad.is_empty(), "oml-web reaches workspace crates: {bad:#?}");
}

#[test]
fn the_wall_walk_sees_oml_page_and_axum() {
    // A walk that reaches nothing would pass both tests above vacuously.
    let names: BTreeSet<String> = reached(&graph()).into_iter().map(|(n, _)| n).collect();
    assert!(names.contains("oml-page"), "{names:?}");
    assert!(names.contains("axum"), "{names:?}");
}
