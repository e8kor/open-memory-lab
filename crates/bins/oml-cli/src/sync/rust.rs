//! The `rust` adapter (ADR-0061): one claim per crate, from its manifest and
//! its root docs, and one per module, from the doc at the head of its file.
//! A pure function of every file under the source (d4): manifests are parsed
//! from the committed bytes, the module tree is walked from the crate roots
//! through `mod x;` declarations, and no line of code reaches a claim.

use std::collections::{BTreeMap, BTreeSet};

use super::adapter::{Adapter, Extraction, Item, SetExtraction, fit_key};
use super::hash::{
    HASHER_CRATE, HASHER_RUSTDOC, crate_hash, md_normal, nfc, rustdoc_hash, rustdoc_text,
};
use super::rustdoc::{DocBlock, MAX_DOC_BYTES, lex, skip_string};
use super::source::glob_match;

/// The largest `.rs` file read, in bytes (PR 88 review); a larger one is
/// skipped by name, its claims neither written nor retired.
pub const MAX_SOURCE_BYTES: usize = 1024 * 1024;
/// The largest `Cargo.toml` read, in bytes; a larger one makes its crate
/// unreadable by name.
pub const MAX_MANIFEST_BYTES: usize = 256 * 1024;

pub struct RustAdapter;

impl Adapter for RustAdapter {
    fn name(&self) -> &'static str {
        "rust"
    }

    fn accepts(&self, path: &str) -> bool {
        path.ends_with(".rs") || path.rsplit('/').next() == Some("Cargo.toml")
    }

    fn extract(&self, path: &str, _bytes: &[u8]) -> Result<Extraction, String> {
        Err(format!(
            "{path}: the rust adapter reads a set of files, not one (ADR-0061 d4)"
        ))
    }

    fn extract_set(&self, files: &[(String, Vec<u8>)]) -> SetExtraction {
        extract_set(files)
    }
}

/// One package of the source.
struct Package {
    /// The directory of its manifest, `""` at the source's root.
    dir: String,
    name: String,
    description: Option<String>,
    lib_name: String,
    lib_root: Option<String>,
    /// Binary name and root file.
    bins: Vec<(String, String)>,
    /// Every dependency's real package name, workspace or not.
    deps: BTreeSet<String>,
}

fn under(dir: &str, path: &str) -> bool {
    dir.is_empty() || path.strip_prefix(dir).is_some_and(|r| r.starts_with('/'))
}

fn join(dir: &str, rel: &str) -> String {
    let rel = rel.trim_start_matches("./");
    if dir.is_empty() {
        rel.to_owned()
    } else {
        format!("{dir}/{rel}")
    }
}

fn parent(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(d, _)| d)
}

fn str_of<'a>(t: &'a toml::Table, key: &str) -> Option<&'a str> {
    t.get(key).and_then(toml::Value::as_str)
}

fn extract_set(files: &[(String, Vec<u8>)]) -> SetExtraction {
    let mut sorted: Vec<&(String, Vec<u8>)> = files.iter().collect();
    sorted.sort_by(|a, b| a.0.cmp(&b.0));
    let bytes: BTreeMap<&str, &[u8]> = sorted
        .iter()
        .map(|(p, b)| (p.as_str(), b.as_slice()))
        .collect();
    let mut out = SetExtraction::default();
    let mut per_file: BTreeMap<String, Extraction> = BTreeMap::new();

    // Manifests.
    let mut tables: Vec<(String, toml::Table)> = Vec::new();
    for (path, b) in &bytes {
        if path.rsplit('/').next() != Some("Cargo.toml") {
            continue;
        }
        match (if b.len() > MAX_MANIFEST_BYTES {
            Err(format!(
                "{path} is too large ({} bytes; the cap is {MAX_MANIFEST_BYTES})",
                b.len()
            ))
        } else {
            std::str::from_utf8(b).map_err(|e| format!("{path} is not UTF-8 ({e})"))
        })
        .and_then(|t| {
            t.parse::<toml::Table>()
                .map_err(|e| format!("{path} does not parse: {e}"))
        }) {
            Ok(t) => tables.push(((*path).to_owned(), t)),
            Err(why) => {
                // Everything under a manifest that cannot be read is skipped,
                // never retired.
                let dir = parent(path).to_owned();
                out.unreadable.push(((*path).to_owned(), why.clone()));
                for p in bytes
                    .keys()
                    .filter(|p| p.ends_with(".rs") && under(&dir, p))
                {
                    out.unreadable.push((
                        (*p).to_owned(),
                        format!("its crate's manifest {path} is unreadable: {why}"),
                    ));
                }
            }
        }
    }
    let workspace_description: Option<String> = tables.iter().find_map(|(_, t)| {
        t.get("workspace")
            .and_then(|w| w.get("package"))
            .and_then(|p| p.get("description"))
            .and_then(toml::Value::as_str)
            .map(ToOwned::to_owned)
    });
    let roots: Vec<WorkspaceRoot> = tables
        .iter()
        .filter_map(|(path, t)| {
            let w = t.get("workspace")?.as_table()?;
            let list = |k: &str| -> Vec<String> {
                w.get(k)
                    .and_then(toml::Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(toml::Value::as_str)
                    .map(ToOwned::to_owned)
                    .collect()
            };
            Some(WorkspaceRoot {
                dir: parent(path).to_owned(),
                members: list("members"),
                exclude: list("exclude"),
                deps: w
                    .get("dependencies")
                    .and_then(toml::Value::as_table)
                    .cloned()
                    .unwrap_or_default(),
            })
        })
        .collect();
    // The nearest workspace root at or above a directory.
    let root_of = |dir: &str| -> Option<&WorkspaceRoot> {
        roots
            .iter()
            .filter(|r| r.dir == dir || under(&r.dir, dir))
            .max_by_key(|r| r.dir.len())
    };
    let members = member_manifests(&tables, &root_of);
    let mut packages: Vec<Package> = Vec::new();
    for (path, t) in &tables {
        let Some(p) = t.get("package").and_then(toml::Value::as_table) else {
            continue;
        };
        if !members.contains(path) {
            out.skipped.push((
                path.clone(),
                "not a workspace member: its workspace's members and exclude leave it out, so it is not a crate of this source (ADR-0061 d1)".to_owned(),
            ));
            continue;
        }
        let Some(name) = str_of(p, "name") else {
            out.unreadable
                .push((path.clone(), format!("{path}: [package] has no name")));
            continue;
        };
        let dir = parent(path).to_owned();
        let description = match p.get("description") {
            Some(toml::Value::String(s)) => Some(s.clone()),
            Some(toml::Value::Table(d))
                if d.get("workspace").and_then(toml::Value::as_bool) == Some(true) =>
            {
                if workspace_description.is_none() {
                    out.skipped.push((
                        path.clone(),
                        format!("unresolved: {name}'s description is the workspace's, which is not under the source; its claim is written without one"),
                    ));
                }
                workspace_description.clone()
            }
            _ => None,
        };
        let lib = t.get("lib").and_then(toml::Value::as_table);
        let lib_name = lib
            .and_then(|l| str_of(l, "name"))
            .map_or_else(|| name.replace('-', "_"), str::to_owned);
        let lib_path = lib
            .and_then(|l| str_of(l, "path"))
            .map_or_else(|| join(&dir, "src/lib.rs"), |p| join(&dir, p));
        let lib_root = bytes.contains_key(lib_path.as_str()).then_some(lib_path);
        let mut bins: Vec<(String, String)> = Vec::new();
        if let Some(list) = t.get("bin").and_then(toml::Value::as_array) {
            for b in list.iter().filter_map(toml::Value::as_table) {
                let Some(bin) = str_of(b, "name") else {
                    continue;
                };
                let root = match str_of(b, "path") {
                    Some(p) => join(&dir, p),
                    None if bin == name
                        && bytes.contains_key(join(&dir, "src/main.rs").as_str()) =>
                    {
                        join(&dir, "src/main.rs")
                    }
                    None => join(&dir, &format!("src/bin/{bin}.rs")),
                };
                bins.push((bin.to_owned(), root));
            }
        }
        if p.get("autobins").and_then(toml::Value::as_bool) != Some(false) {
            let main = join(&dir, "src/main.rs");
            if bytes.contains_key(main.as_str()) && !bins.iter().any(|(_, r)| *r == main) {
                bins.push((name.to_owned(), main));
            }
            let prefix = join(&dir, "src/bin/");
            for f in bytes.keys().filter_map(|f| f.strip_prefix(prefix.as_str())) {
                let root = format!("{prefix}{f}");
                let bin = match f.split_once('/') {
                    None => f.strip_suffix(".rs"),
                    Some((d, "main.rs")) => Some(d),
                    Some(_) => None,
                };
                if let Some(bin) = bin
                    && !bins.iter().any(|(_, r)| *r == root)
                {
                    bins.push((bin.to_owned(), root));
                }
            }
        }
        bins.retain(|(_, r)| bytes.contains_key(r.as_str()));
        bins.sort();
        let mut deps = BTreeSet::new();
        let inherited = root_of(&dir).map(|r| &r.deps);
        let mut collect = |table: &toml::Table| {
            for (key, v) in table {
                deps.insert(real_package(key, v, inherited).to_owned());
            }
        };
        if let Some(d) = t.get("dependencies").and_then(toml::Value::as_table) {
            collect(d);
        }
        if let Some(targets) = t.get("target").and_then(toml::Value::as_table) {
            for cfg in targets.values().filter_map(toml::Value::as_table) {
                if let Some(d) = cfg.get("dependencies").and_then(toml::Value::as_table) {
                    collect(d);
                }
            }
        }
        packages.push(Package {
            dir,
            name: name.to_owned(),
            description,
            lib_name,
            lib_root,
            bins,
            deps,
        });
    }
    if packages.is_empty() && out.unreadable.is_empty() {
        out.unreadable.push((
            ".".to_owned(),
            "the source holds no [package] Cargo.toml, so there is no crate to read; \
             name a directory under a Cargo workspace (ADR-0061 d1)"
                .to_owned(),
        ));
    }

    let names: BTreeSet<String> = packages.iter().map(|p| p.name.clone()).collect();
    let internal = |p: &Package| -> BTreeSet<String> {
        p.deps
            .iter()
            .filter(|d| names.contains(*d) && **d != p.name)
            .cloned()
            .collect()
    };
    let mut dependents: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for p in &packages {
        for d in internal(p) {
            dependents.entry(d).or_default().insert(p.name.clone());
        }
    }

    // The owner of each .rs file under a package's src/: the package with the
    // longest manifest directory.
    let owner_of = |path: &str| -> Option<usize> {
        packages
            .iter()
            .enumerate()
            .filter(|(_, p)| under(&p.dir, path) && under(&join(&p.dir, "src"), path))
            .max_by_key(|(_, p)| p.dir.len())
            .map(|(i, _)| i)
    };

    let mut roots_of: Vec<BTreeMap<String, Docs>> = Vec::new();
    let mut reached: BTreeSet<String> = BTreeSet::new();
    let mut gated: BTreeSet<String> = BTreeSet::new();
    let mut crate_bad: Vec<Option<String>> = vec![None; packages.len()];
    let mut modules: Vec<(usize, String, String)> = Vec::new(); // (package, path, file)

    for (idx, pkg) in packages.iter().enumerate() {
        let mut roots: Vec<(String, String, bool)> = Vec::new(); // (module root name, file, is lib)
        if let Some(r) = &pkg.lib_root {
            roots.push((pkg.lib_name.clone(), r.clone(), true));
        }
        for (bin, r) in &pkg.bins {
            roots.push((bin.replace('-', "_"), r.clone(), false));
        }
        let mut docs: BTreeMap<String, Docs> = BTreeMap::new();
        for (root_name, root_file, _) in &roots {
            let Some(b) = bytes.get(root_file.as_str()) else {
                continue;
            };
            docs.insert(root_file.clone(), read_docs(root_file, b));
            let mut stack = vec![(
                root_file.clone(),
                root_name.clone(),
                parent(root_file).to_owned(),
            )];
            while let Some((file, mpath, dir)) = stack.pop() {
                reached.insert(file.clone());
                let Some(b) = bytes.get(file.as_str()) else {
                    continue;
                };
                if b.len() > MAX_SOURCE_BYTES {
                    continue;
                }
                let Ok(text) = std::str::from_utf8(b) else {
                    continue;
                };
                let decls = scan_mods(text);
                if let Some(d) = decls
                    .iter()
                    .find(|d| d.attrs.iter().any(|a| is_path_attr(a)))
                {
                    crate_bad[idx] = Some(format!(
                        "unsupported: #[path = …] on `mod {}` in {file}, which this adapter does not follow (ADR-0061 d3)",
                        d.name
                    ));
                }
                for d in decls {
                    let test = d
                        .attrs
                        .iter()
                        .any(|a| a.split_whitespace().collect::<String>() == "cfg(test)");
                    let mut base = dir.clone();
                    let mut mp = mpath.clone();
                    for seg in &d.inline {
                        base = join(&base, seg);
                        mp = format!("{mp}::{seg}");
                    }
                    let found = [
                        join(&base, &format!("{}.rs", d.name)),
                        join(&join(&base, &d.name), "mod.rs"),
                    ]
                    .into_iter()
                    .find(|c| bytes.contains_key(c.as_str()));
                    let Some(child) = found else { continue };
                    let child_dir = if child.ends_with("/mod.rs") {
                        parent(&child).to_owned()
                    } else {
                        join(&base, &d.name)
                    };
                    if test {
                        gated.insert(child.clone());
                        // Everything below a test module is test code too.
                        for f in bytes
                            .keys()
                            .filter(|f| f.ends_with(".rs") && under(&child_dir, f))
                        {
                            gated.insert((*f).to_owned());
                        }
                        continue;
                    }
                    if reached.contains(&child) {
                        continue;
                    }
                    let child_path = format!("{mp}::{}", d.name);
                    modules.push((idx, child_path.clone(), child.clone()));
                    stack.push((child, child_path, child_dir));
                }
            }
        }
        roots_of.push(docs);
    }

    // Crate claims.
    for (idx, pkg) in packages.iter().enumerate() {
        let manifest = join(&pkg.dir, "Cargo.toml");
        if let Some(why) = &crate_bad[idx] {
            out.unreadable.push((manifest.clone(), why.clone()));
            continue;
        }
        let docs = &roots_of[idx];
        let lib = pkg.lib_root.as_ref().and_then(|r| docs.get(r));
        let first_bin = pkg.bins.first().and_then(|(_, r)| docs.get(r));
        let mut unreadable_root = None;
        let mut skipped_root = None;
        for r in pkg.lib_root.iter().chain(pkg.bins.iter().map(|(_, r)| r)) {
            match docs.get(r) {
                Some(Docs::Unreadable(why)) => unreadable_root = Some((r.clone(), why.clone())),
                Some(Docs::TooLarge(why)) => skipped_root = Some((r.clone(), why.clone())),
                Some(Docs::Unterminated) => per_file
                    .entry(r.clone())
                    .or_default()
                    .warnings
                    .push(unterminated_warning(r)),
                _ => {}
            }
        }
        if let Some((root, why)) = skipped_root
            && unreadable_root.is_none()
        {
            out.skipped.push((
                manifest.clone(),
                format!("{why}: its root {root} is not read, so the crate claim is not written"),
            ));
            continue;
        }
        if let Some((root, why)) = unreadable_root {
            out.unreadable.push((
                manifest.clone(),
                format!("its root {root} cannot be read, so the crate claim is not written: {why}"),
            ));
            continue;
        }
        let root_doc = lib.or(first_bin).and_then(Docs::block);
        let mut text = match &pkg.description {
            Some(d) => format!("Crate {}: {}", pkg.name, d.trim()),
            None => format!("Crate {}.", pkg.name),
        };
        if let Some(d) = root_doc {
            text.push_str("\n\n");
            text.push_str(&d.text());
        }
        let bin_facts: Vec<(String, String)> = pkg
            .bins
            .iter()
            .map(|(n, r)| {
                (
                    n.clone(),
                    docs.get(r)
                        .and_then(Docs::block)
                        .map(DocBlock::first_paragraph)
                        .unwrap_or_default(),
                )
            })
            .collect();
        if !bin_facts.is_empty() {
            let list: Vec<String> = bin_facts
                .iter()
                .map(|(n, p)| {
                    if p.is_empty() {
                        n.clone()
                    } else {
                        format!("{n} — {p}")
                    }
                })
                .collect();
            text.push_str(&format!("\n\nBinaries: {}", list.join("; ")));
        }
        let deps = internal(pkg);
        let rdeps = dependents.get(&pkg.name).cloned().unwrap_or_default();
        let mut tail = Vec::new();
        if !deps.is_empty() {
            tail.push(format!(
                "Depends on (in this workspace): {}.",
                deps.iter().cloned().collect::<Vec<_>>().join(", ")
            ));
        }
        if !rdeps.is_empty() {
            tail.push(format!(
                "Depended on by (in this workspace): {}.",
                rdeps.iter().cloned().collect::<Vec<_>>().join(", ")
            ));
        }
        if !tail.is_empty() {
            text.push_str(&format!("\n\n{}", tail.join("\n")));
        }
        let mut facts = vec![
            pkg.name.clone(),
            md_normal(pkg.description.as_deref().unwrap_or_default()),
            md_normal(&root_doc.map(|d| rustdoc_text(&d.lines)).unwrap_or_default()),
        ];
        facts.extend(
            bin_facts
                .iter()
                .map(|(n, p)| format!("bin {n} {}", md_normal(p))),
        );
        facts.extend(deps.iter().map(|d| format!("dep {d}")));
        facts.extend(rdeps.iter().map(|d| format!("rdep {d}")));
        per_file.entry(manifest).or_default().items.push(Item {
            claim_key: fit_key(&format!("rust/crate/{}", pkg.name)),
            fragment: None,
            text: nfc(&text),
            lines: (1, 1),
            hash: crate_hash(&facts),
            hasher: HASHER_CRATE,
        });
    }

    // Module claims.
    for (idx, mpath, file) in &modules {
        let pkg = &packages[*idx];
        if let Some(why) = &crate_bad[*idx] {
            out.unreadable.push((file.clone(), why.clone()));
            continue;
        }
        let Some(b) = bytes.get(file.as_str()) else {
            continue;
        };
        match read_docs(file, b) {
            Docs::Unreadable(why) => out.unreadable.push((file.clone(), why)),
            Docs::Generated => out.skipped.push((
                file.clone(),
                "generated: its first line carries @generated".to_owned(),
            )),
            Docs::TooLarge(why) => out.skipped.push((file.clone(), why)),
            Docs::Unterminated => per_file
                .entry(file.clone())
                .or_default()
                .warnings
                .push(unterminated_warning(file)),
            Docs::None => per_file
                .entry(file.clone())
                .or_default()
                .warnings
                .push(format!(
                    "{file} has no leading //! doc, so it yields no claim"
                )),
            Docs::Block(d) => {
                let doc = d.text();
                if doc.is_empty() {
                    continue;
                }
                per_file.entry(file.clone()).or_default().items.push(Item {
                    claim_key: fit_key(&format!("rust/mod/{mpath}")),
                    fragment: Some(format!("L{}-L{}", d.first, d.last)),
                    text: nfc(&format!("Module {mpath} of crate {}.\n\n{doc}", pkg.name)),
                    lines: (d.first, d.last),
                    hash: rustdoc_hash(&d.lines),
                    hasher: HASHER_RUSTDOC,
                });
            }
        }
    }
    // A broken crate's other files, and every file under a src/ that no root
    // reaches.
    let root_files: BTreeSet<&String> = packages
        .iter()
        .flat_map(|p| p.lib_root.iter().chain(p.bins.iter().map(|(_, r)| r)))
        .collect();
    for path in bytes.keys().filter(|p| p.ends_with(".rs")) {
        let Some(idx) = owner_of(path) else { continue };
        let path = (*path).to_owned();
        if let Some(why) = &crate_bad[idx]
            && !out.unreadable.iter().any(|(p, _)| *p == path)
        {
            out.unreadable.push((path, why.clone()));
            continue;
        }
        if reached.contains(&path) || gated.contains(&path) || root_files.contains(&path) {
            continue;
        }
        if out.unreadable.iter().any(|(p, _)| *p == path) {
            continue;
        }
        let bin_dir = join(&packages[idx].dir, "src/bin/");
        if path.starts_with(&bin_dir) {
            continue;
        }
        out.skipped.push((
            path,
            "undeclared: no `mod` declaration reaches it, so it is not compiled (ADR-0061 d1)"
                .to_owned(),
        ));
    }
    out.files = per_file.into_iter().collect();
    out.unreadable.sort();
    out.unreadable.dedup();
    out.skipped.sort();
    out
}

/// What a root or module file's head holds.
enum Docs {
    Block(DocBlock),
    None,
    Generated,
    Unreadable(String),
    /// A `/*!` block or `#![` attribute that never closes: no doc.
    Unterminated,
    /// A file or doc over its cap, by name.
    TooLarge(String),
}

fn unterminated_warning(path: &str) -> String {
    format!(
        "{path} has an unterminated /*! block or #![ attribute at its head, so it yields no claim"
    )
}

impl Docs {
    fn block(&self) -> Option<&DocBlock> {
        match self {
            Self::Block(d) => Some(d),
            _ => None,
        }
    }
}

fn read_docs(path: &str, bytes: &[u8]) -> Docs {
    if bytes.len() > MAX_SOURCE_BYTES {
        return Docs::TooLarge(format!(
            "too large: {path} is {} bytes, over the {MAX_SOURCE_BYTES}-byte cap",
            bytes.len()
        ));
    }
    let text = match std::str::from_utf8(bytes) {
        Ok(t) => t,
        Err(e) => return Docs::Unreadable(format!("{path} is not UTF-8 ({e})")),
    };
    if text
        .lines()
        .next()
        .is_some_and(|l| l.contains("@generated"))
    {
        return Docs::Generated;
    }
    match lex(text) {
        Ok(l) if l.unterminated => Docs::Unterminated,
        Ok(l) => match l.doc {
            Some(d) if d.lines.iter().map(String::len).sum::<usize>() > MAX_DOC_BYTES => {
                Docs::TooLarge(format!(
                    "too large: the leading doc of {path} is over the {MAX_DOC_BYTES}-byte cap"
                ))
            }
            Some(d) => Docs::Block(d),
            None => Docs::None,
        },
        Err(why) => Docs::Unreadable(why),
    }
}

/// A workspace's root manifest: its members, excludes and shared
/// dependencies.
struct WorkspaceRoot {
    dir: String,
    members: Vec<String>,
    exclude: Vec<String>,
    deps: toml::Table,
}

/// The real package a dependency entry names: its `package =` rename, or the
/// workspace table's entry for `foo.workspace = true`, or its key.
fn real_package<'a>(key: &'a str, v: &'a toml::Value, shared: Option<&'a toml::Table>) -> &'a str {
    let Some(d) = v.as_table() else { return key };
    if d.get("workspace").and_then(toml::Value::as_bool) == Some(true)
        && let Some(w) = shared
            .and_then(|s| s.get(key))
            .and_then(toml::Value::as_table)
    {
        return str_of(w, "package").unwrap_or(key);
    }
    str_of(d, "package").unwrap_or(key)
}

/// `a/./b/../c` as `a/c`.
fn normalize(path: &str) -> String {
    let mut out: Vec<&str> = Vec::new();
    for seg in path.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                out.pop();
            }
            s => out.push(s),
        }
    }
    out.join("/")
}

/// The dependency tables of a manifest, target tables included.
fn dep_tables(t: &toml::Table) -> Vec<&toml::Table> {
    fn take<'a>(t: &'a toml::Table, out: &mut Vec<&'a toml::Table>) {
        for k in ["dependencies", "dev-dependencies", "build-dependencies"] {
            if let Some(d) = t.get(k).and_then(toml::Value::as_table) {
                out.push(d);
            }
        }
    }
    let mut out = Vec::new();
    take(t, &mut out);
    if let Some(targets) = t.get("target").and_then(toml::Value::as_table) {
        for cfg in targets.values().filter_map(toml::Value::as_table) {
            take(cfg, &mut out);
        }
    }
    out
}

/// The manifests that are crates of the source: a package outside every
/// workspace root in the source, a root package, a member the root's globs
/// list and its `exclude` does not remove, and any path dependency of a
/// member that sits inside the root's directory and is not excluded (as
/// cargo does).
fn member_manifests<'a>(
    tables: &[(String, toml::Table)],
    root_of: &dyn Fn(&str) -> Option<&'a WorkspaceRoot>,
) -> BTreeSet<String> {
    let has_package = |t: &toml::Table| t.get("package").is_some_and(toml::Value::is_table);
    let excluded =
        |r: &WorkspaceRoot, dir: &str| r.exclude.iter().any(|g| glob_match(&join(&r.dir, g), dir));
    let mut members: BTreeSet<String> = BTreeSet::new();
    for (path, _) in tables.iter().filter(|(_, t)| has_package(t)) {
        let dir = parent(path);
        let ok = match root_of(dir) {
            None => true,
            Some(r) if r.dir == dir => true,
            Some(r) => {
                r.members.iter().any(|g| glob_match(&join(&r.dir, g), dir)) && !excluded(r, dir)
            }
        };
        if ok {
            members.insert(path.clone());
        }
    }
    // Path dependencies pull their packages in, transitively.
    loop {
        let mut added = Vec::new();
        for (path, t) in tables.iter().filter(|(p, _)| members.contains(p)) {
            let dir = parent(path);
            let root = root_of(dir);
            for table in dep_tables(t) {
                for (key, v) in table {
                    let Some(d) = v.as_table() else { continue };
                    let target = if let Some(p) = str_of(d, "path") {
                        normalize(&join(dir, p))
                    } else if d.get("workspace").and_then(toml::Value::as_bool) == Some(true)
                        && let Some(r) = root
                        && let Some(p) = r
                            .deps
                            .get(key)
                            .and_then(toml::Value::as_table)
                            .and_then(|w| str_of(w, "path"))
                    {
                        normalize(&join(&r.dir, p))
                    } else {
                        continue;
                    };
                    let manifest = join(&target, "Cargo.toml");
                    let inside = root.is_none_or(|r| r.dir.is_empty() || under(&r.dir, &target))
                        && root.is_none_or(|r| !excluded(r, &target));
                    if inside
                        && !members.contains(&manifest)
                        && tables.iter().any(|(p, t)| *p == manifest && has_package(t))
                    {
                        added.push(manifest);
                    }
                }
            }
        }
        if added.is_empty() {
            return members;
        }
        members.extend(added);
    }
}

fn is_path_attr(attr: &str) -> bool {
    let a = attr.trim_start();
    a.strip_prefix("path")
        .is_some_and(|r| r.trim_start().starts_with('='))
}

/// One `mod name;` declaration at module level.
struct ModDecl {
    name: String,
    /// The inline modules it sits in, outermost first.
    inline: Vec<String>,
    /// The text of the attributes before it.
    attrs: Vec<String>,
}

#[derive(Debug)]
enum Tok {
    Ident(String),
    Punct(u8),
    Attr(String),
}

/// A token stream of `src` without comments, strings or literals: enough to
/// find `mod name;` and the attributes before it.
fn tokens(src: &str) -> Vec<Tok> {
    let b = src.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if c.is_ascii_whitespace() {
            i += 1;
        } else if b[i..].starts_with(b"//") {
            while i < b.len() && b[i] != b'\n' {
                i += 1;
            }
        } else if b[i..].starts_with(b"/*") {
            let mut depth = 0;
            while i < b.len() {
                if b[i..].starts_with(b"/*") {
                    depth += 1;
                    i += 2;
                } else if b[i..].starts_with(b"*/") {
                    depth -= 1;
                    i += 2;
                    if depth == 0 {
                        break;
                    }
                } else {
                    i += 1;
                }
            }
        } else if c == b'#' && (b[i..].starts_with(b"#[") || b[i..].starts_with(b"#![")) {
            let open = if b[i + 1] == b'!' { i + 2 } else { i + 1 };
            let mut depth = 0;
            let mut j = open;
            while j < b.len() {
                match b[j] {
                    b'[' => depth += 1,
                    b']' => {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                    }
                    b'"' => {
                        j = skip_string(b, j);
                        continue;
                    }
                    _ => {}
                }
                j += 1;
            }
            out.push(Tok::Attr(src[open + 1..j.min(b.len())].to_owned()));
            i = j + 1;
        } else if c == b'"' {
            i = skip_string(b, i);
        } else if (c == b'r' || c == b'b') && raw_string_end(b, i).is_some() {
            i = raw_string_end(b, i).unwrap_or(b.len());
        } else if c == b'\'' {
            // A char literal, or a lifetime.
            if b.get(i + 1) == Some(&b'\\') {
                i += 2;
                while i < b.len() && b[i] != b'\'' {
                    i += 1;
                }
                i += 1;
            } else if let Some(ch) = src[i + 1..].chars().next()
                && b.get(i + 1 + ch.len_utf8()) == Some(&b'\'')
            {
                i += 2 + ch.len_utf8();
            } else {
                i += 1;
            }
        } else if c.is_ascii_alphabetic() || c == b'_' {
            let start = i;
            while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_') {
                i += 1;
            }
            out.push(Tok::Ident(src[start..i].to_owned()));
        } else if c.is_ascii_digit() {
            while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_' || b[i] == b'.') {
                i += 1;
            }
        } else {
            out.push(Tok::Punct(c));
            i += 1;
        }
    }
    out
}

/// The index after a raw or byte string literal starting at `i`
/// (`r"…"`, `r#"…"#`, `b"…"`, `br#"…"#`), `None` if `i` starts none.
fn raw_string_end(b: &[u8], i: usize) -> Option<usize> {
    let mut j = i;
    if b[j] == b'b' {
        j += 1;
    }
    if b.get(j) == Some(&b'"') && j > i {
        return Some(skip_string(b, j));
    }
    if b.get(j) != Some(&b'r') {
        return None;
    }
    j += 1;
    let hashes = b[j..].iter().take_while(|&&c| c == b'#').count();
    j += hashes;
    if b.get(j) != Some(&b'"') {
        return None;
    }
    let close: Vec<u8> = std::iter::once(b'"')
        .chain(std::iter::repeat_n(b'#', hashes))
        .collect();
    let rest = &b[j + 1..];
    Some(
        rest.windows(close.len())
            .position(|w| w == close.as_slice())
            .map_or(b.len(), |p| j + 1 + p + close.len()),
    )
}

/// The `mod name;` declarations at module level: not inside a function, an
/// `impl` or a macro, and inside inline `mod name { … }` blocks only.
fn scan_mods(src: &str) -> Vec<ModDecl> {
    let toks = tokens(src);
    let mut out = Vec::new();
    // One entry per open brace: `Some(name)` for an inline module.
    let mut stack: Vec<Option<String>> = Vec::new();
    let mut i = 0;
    while i < toks.len() {
        match &toks[i] {
            Tok::Punct(b'{') => stack.push(None),
            Tok::Punct(b'}') => {
                stack.pop();
            }
            Tok::Ident(kw) if kw == "mod" && stack.iter().all(Option::is_some) => {
                let after_path = i > 0 && matches!(toks[i - 1], Tok::Punct(b':' | b'.'));
                if let (false, Some(Tok::Ident(name))) = (after_path, toks.get(i + 1)) {
                    match toks.get(i + 2) {
                        Some(Tok::Punct(b';')) => {
                            out.push(ModDecl {
                                name: name.clone(),
                                inline: stack.iter().flatten().cloned().collect(),
                                attrs: attrs_before(&toks, i),
                            });
                            i += 2;
                        }
                        Some(Tok::Punct(b'{')) => {
                            stack.push(Some(name.clone()));
                            i += 2;
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
        i += 1;
    }
    out
}

/// The attributes directly before the item whose `mod` keyword is at `at`,
/// past its visibility.
fn attrs_before(toks: &[Tok], at: usize) -> Vec<String> {
    let mut i = at;
    if i > 0 && matches!(toks[i - 1], Tok::Punct(b')')) {
        while i > 0 && !matches!(toks[i - 1], Tok::Punct(b'(')) {
            i -= 1;
        }
        i = i.saturating_sub(1);
    }
    if i > 0 && matches!(&toks[i - 1], Tok::Ident(p) if p == "pub") {
        i -= 1;
    }
    let mut attrs = Vec::new();
    while i > 0 {
        match &toks[i - 1] {
            Tok::Attr(a) => attrs.push(a.clone()),
            _ => break,
        }
        i -= 1;
    }
    attrs
}
