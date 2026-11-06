//! The `rust` adapter of `oml sync` (ADR-0061): golden against this
//! repository's own crates (their manifests, and each source file's leading
//! docs and `mod` declarations, copied into `tests/fixtures/sync/rust` at
//! `aee5b28`), the prefix lexer, the claims' keys, anchors and hashes.

use std::path::{Path, PathBuf};

use oml_cli::sync::adapter::{Adapter, Item, Kind, SetExtraction};
use oml_cli::sync::rust::RustAdapter;
use oml_cli::sync::rustdoc::leading_docs;

fn fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/sync/rust")
}

/// Every file under the fixture, `Cargo.toml.fixture` read as `Cargo.toml`.
fn fixture_files() -> Vec<(String, Vec<u8>)> {
    fn walk(dir: &Path, root: &Path, out: &mut Vec<(String, Vec<u8>)>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let p = entry.unwrap().path();
            if p.is_dir() {
                walk(&p, root, out);
            } else {
                let rel = p.strip_prefix(root).unwrap().to_str().unwrap().to_owned();
                let rel = rel
                    .strip_suffix(".fixture")
                    .map_or(rel.clone(), ToOwned::to_owned);
                out.push((rel, std::fs::read(&p).unwrap()));
            }
        }
    }
    let mut out = Vec::new();
    walk(&fixture_dir(), &fixture_dir(), &mut out);
    out.sort();
    out
}

fn files(list: &[(&str, &str)]) -> Vec<(String, Vec<u8>)> {
    list.iter()
        .map(|(p, t)| ((*p).to_owned(), t.as_bytes().to_vec()))
        .collect()
}

fn items(x: &SetExtraction) -> Vec<&Item> {
    x.files.iter().flat_map(|(_, e)| &e.items).collect()
}

fn get<'a>(x: &'a SetExtraction, key: &str) -> &'a Item {
    items(x)
        .into_iter()
        .find(|i| i.claim_key == key)
        .unwrap_or_else(|| {
            panic!(
                "no item {key}: {:?}",
                items(x).iter().map(|i| &i.claim_key).collect::<Vec<_>>()
            )
        })
}

fn extract(list: &[(&str, &str)]) -> SetExtraction {
    RustAdapter.extract_set(&files(list))
}

const A_MANIFEST: &str = r#"[package]
name = "oml-a"
description = "Crate A."

[dependencies]
oml-b.workspace = true
serde = "1"

[dev-dependencies]
oml-c.workspace = true
"#;
const B_MANIFEST: &str =
    "[package]\nname = \"oml-b\"\ndescription = \"Crate B.\"\n\n[lib]\nname = \"bee\"\n";
const A_LIB: &str = "//! A root.\n\npub mod m;\nmod hidden;\n";
const A_M: &str = "//! Module m.\n//!\n//! More about m.\n\npub fn f() {}\n";
const A_HIDDEN: &str = "//! Hidden module.\n";
const B_LIB: &str = "//! B root.\n\npub mod deep;\n";
const B_DEEP: &str = "//! Deep.\n";

fn ws() -> Vec<(&'static str, &'static str)> {
    vec![
        ("crates/a/Cargo.toml", A_MANIFEST),
        ("crates/a/src/lib.rs", A_LIB),
        ("crates/a/src/m.rs", A_M),
        ("crates/a/src/hidden/mod.rs", A_HIDDEN),
        ("crates/b/Cargo.toml", B_MANIFEST),
        ("crates/b/src/lib.rs", B_LIB),
        ("crates/b/src/deep.rs", B_DEEP),
    ]
}

fn with(edit: &str, text: &str) -> SetExtraction {
    let list: Vec<(&str, &str)> = ws()
        .into_iter()
        .map(|(p, t)| if p == edit { (p, text) } else { (p, t) })
        .collect();
    extract(&list)
}

#[test]
fn the_rust_adapter_at_aee5b28_yields_9_crates_and_98_modules() {
    // The counts come from a script that walks `mod` declarations from each
    // crate root with a regular expression, never from the adapter.
    let x = RustAdapter.extract_set(&fixture_files());
    assert_eq!(x.unreadable, Vec::new(), "{:?}", x.unreadable);
    assert_eq!(x.skipped, Vec::new(), "{:?}", x.skipped);
    let all = items(&x);
    let crates = all
        .iter()
        .filter(|i| i.claim_key.starts_with("rust/crate/"))
        .count();
    let modules = all
        .iter()
        .filter(|i| i.claim_key.starts_with("rust/mod/"))
        .count();
    assert_eq!((crates, modules), (9, 98));
    let keys: std::collections::BTreeSet<_> = all.iter().map(|i| &i.claim_key).collect();
    assert_eq!(keys.len(), 107, "every key is unique");
    for key in [
        "rust/crate/oml-domain",
        "rust/crate/oml-cli",
        "rust/mod/oml_domain::guardrail",
        "rust/mod/oml_app::ports",
        "rust/mod/oml_cli::sync::plan",
    ] {
        assert!(keys.contains(&key.to_owned()), "{key}");
    }
    assert!(
        all.iter().all(|i| i.claim_key.len() <= 128),
        "every key fits the grammar"
    );
}

#[test]
fn the_golden_load_is_the_same_in_any_file_order() {
    let mut files = fixture_files();
    let forward = RustAdapter.extract_set(&files);
    files.reverse();
    assert_eq!(forward, RustAdapter.extract_set(&files));
}

#[test]
fn the_golden_crate_claim_of_oml_app_names_its_dependency_and_its_dependents() {
    let x = RustAdapter.extract_set(&fixture_files());
    let c = get(&x, "rust/crate/oml-app");
    assert!(
        c.text
            .starts_with("Crate oml-app: Port traits and use-cases")
    );
    assert!(
        c.text
            .contains("Depends on (in this workspace): oml-domain.")
    );
    assert!(c.text.contains("Depended on by (in this workspace): "));
    assert!(c.text.contains("oml-pg"));
    assert!(!c.text.contains("serde"), "no external names: {}", c.text);
    assert_eq!(c.hasher, "oml-crate/1");
}

#[test]
fn the_lexer_stops_at_the_first_item_and_reads_no_code() {
    let src = "//! first\n//! second\n\nfn main() {\n    //! never a doc\n}\n//! nor this\n";
    let d = leading_docs(src).unwrap().unwrap();
    assert_eq!(d.lines, vec![" first", " second"]);
    assert_eq!((d.first, d.last), (1, 2));
}

#[test]
fn nested_block_comments_and_inner_attributes_before_the_doc_are_skipped() {
    let src = "#![forbid(unsafe_code)]\n/* a /* nested */ comment */\n// line\n#![allow(dead_code, clippy::x = \"]\")]\n//! The doc.\npub fn f() {}\n";
    let d = leading_docs(src).unwrap().unwrap();
    assert_eq!(d.lines, vec![" The doc."]);
    assert_eq!((d.first, d.last), (5, 5));
}

#[test]
fn a_file_with_no_doc_has_none_and_a_shebang_and_a_byte_order_mark_are_skipped() {
    assert_eq!(leading_docs("pub fn f() {}\n").unwrap(), None);
    let d = leading_docs("\u{feff}#!/usr/bin/env rust-script\n//! Script.\n")
        .unwrap()
        .unwrap();
    assert_eq!((d.lines, d.first), (vec![" Script.".to_owned()], 2));
}

#[test]
fn a_block_doc_is_read_as_its_lines() {
    let d = leading_docs("/*! One\n Two */\nfn f() {}\n")
        .unwrap()
        .unwrap();
    assert_eq!((d.first, d.last), (1, 2));
    assert_eq!(
        d.lines.join("\n").split_whitespace().collect::<Vec<_>>(),
        ["One", "Two"]
    );
}

#[test]
fn a_doc_attribute_or_a_path_attribute_is_reported_unsupported() {
    let why = leading_docs("#![doc = \"x\"]\n//! y\n").unwrap_err();
    assert!(why.contains("unsupported"), "{why}");
    let x = extract(&[
        ("crates/a/Cargo.toml", A_MANIFEST),
        (
            "crates/a/src/lib.rs",
            "//! A.\n\n#[path = \"elsewhere.rs\"]\nmod m;\n",
        ),
        ("crates/a/src/elsewhere.rs", "//! Elsewhere.\n"),
    ]);
    assert!(items(&x).is_empty(), "no claim of an unsupported crate");
    let paths: Vec<_> = x.unreadable.iter().map(|(p, _)| p.as_str()).collect();
    assert!(paths.contains(&"crates/a/src/lib.rs"), "{paths:?}");
    assert!(x.unreadable.iter().all(|(_, w)| w.contains("unsupported")));
}

#[test]
fn a_module_path_follows_the_lib_name_and_drops_mod_rs() {
    let x = extract(&ws());
    let keys: Vec<_> = items(&x).iter().map(|i| i.claim_key.clone()).collect();
    assert_eq!(
        keys,
        [
            "rust/crate/oml-a",
            "rust/mod/oml_a::hidden",
            "rust/mod/oml_a::m",
            "rust/crate/oml-b",
            "rust/mod/bee::deep",
        ]
    );
}

#[test]
fn a_module_claim_is_anchored_to_its_doc_span_and_hashed_on_the_doc_alone() {
    let x = extract(&ws());
    let m = get(&x, "rust/mod/oml_a::m");
    assert_eq!(m.fragment.as_deref(), Some("L1-L3"));
    assert_eq!(m.lines, (1, 3));
    assert_eq!(m.hasher, "oml-rustdoc/1");
    assert_eq!(
        m.text,
        "Module oml_a::m of crate oml-a.\n\nModule m.\n\nMore about m."
    );
    let owner = x
        .files
        .iter()
        .find(|(_, e)| e.items.iter().any(|i| i.claim_key == m.claim_key))
        .unwrap();
    assert_eq!(owner.0, "crates/a/src/m.rs");
    assert!(m.hash.starts_with("sha256:") && m.hash.len() == 71);
}

#[test]
fn a_crate_claim_lists_internal_dependencies_and_dependents_and_no_external_names() {
    let x = extract(&ws());
    let a = get(&x, "rust/crate/oml-a");
    let owner = x
        .files
        .iter()
        .find(|(_, e)| e.items.iter().any(|i| i.claim_key == a.claim_key))
        .unwrap();
    assert_eq!(owner.0, "crates/a/Cargo.toml");
    assert_eq!(a.fragment, None);
    assert_eq!(
        a.text,
        "Crate oml-a: Crate A.\n\nA root.\n\nDepends on (in this workspace): oml-b."
    );
    let b = get(&x, "rust/crate/oml-b");
    assert_eq!(
        b.text,
        "Crate oml-b: Crate B.\n\nB root.\n\nDepended on by (in this workspace): oml-a."
    );
    assert!(!a.text.contains("serde") && !a.text.contains("oml-c"));
}

#[test]
fn test_modules_examples_benches_and_build_scripts_are_never_read() {
    let mut list = ws();
    list.extend([
        ("crates/a/tests/t.rs", "//! A test doc.\n"),
        ("crates/a/examples/e.rs", "//! An example doc.\n"),
        ("crates/a/benches/b.rs", "//! A bench doc.\n"),
        ("crates/a/build.rs", "//! A build script.\n"),
    ]);
    list[1] = (
        "crates/a/src/lib.rs",
        "//! A root.\n\npub mod m;\nmod hidden;\n#[cfg(test)]\nmod tests;\n",
    );
    list.push(("crates/a/src/tests.rs", "//! The test module.\n"));
    let x = extract(&list);
    assert_eq!(items(&x).len(), 5);
    assert!(items(&x).iter().all(|i| !i.text.contains("test")));
    assert_eq!(x.unreadable, Vec::new());
    assert_eq!(x.skipped, Vec::new());
}

#[test]
fn an_undeclared_file_is_reported_and_not_extracted() {
    let mut list = ws();
    list.push(("crates/a/src/stray.rs", "//! Stray.\n"));
    let x = extract(&list);
    assert_eq!(items(&x).len(), 5);
    assert_eq!(x.skipped.len(), 1);
    assert_eq!(x.skipped[0].0, "crates/a/src/stray.rs");
    assert!(x.skipped[0].1.contains("undeclared"), "{:?}", x.skipped);
}

#[test]
fn a_generated_file_is_skipped_and_reported() {
    let mut list = ws();
    list[2] = ("crates/a/src/m.rs", "// @generated by a tool\n//! Doc.\n");
    let x = extract(&list);
    assert!(items(&x).iter().all(|i| i.claim_key != "rust/mod/oml_a::m"));
    assert!(
        x.skipped
            .iter()
            .any(|(p, w)| p == "crates/a/src/m.rs" && w.contains("generated")),
        "{:?}",
        x.skipped
    );
}

#[test]
fn no_line_outside_a_doc_block_reaches_any_item_text() {
    let x = extract(&[
        ("crates/a/Cargo.toml", A_MANIFEST),
        (
            "crates/a/src/lib.rs",
            "//! Root.\n\nconst SECRET: &str = \"hunter2\";\n\npub mod m;\n",
        ),
        (
            "crates/a/src/m.rs",
            "// licence header\n//! Doc.\n\npub fn f() { let _ = \"hunter2\"; }\n",
        ),
    ]);
    for i in items(&x) {
        assert!(
            !i.text.contains("hunter2") && !i.text.contains("licence"),
            "{}",
            i.text
        );
    }
}

#[test]
fn a_file_that_is_not_utf8_is_unreadable_and_the_others_are_read() {
    let mut f = files(&ws());
    f[2].1 = vec![0xff, 0xfe, b'/'];
    let x = RustAdapter.extract_set(&f);
    assert_eq!(x.unreadable.len(), 1);
    assert_eq!(x.unreadable[0].0, "crates/a/src/m.rs");
    assert!(
        items(&x)
            .iter()
            .any(|i| i.claim_key == "rust/mod/oml_a::hidden")
    );
    assert!(items(&x).iter().all(|i| i.claim_key != "rust/mod/oml_a::m"));
}

#[test]
fn a_manifest_that_does_not_parse_makes_its_whole_crate_unreadable() {
    let x = extract(&[
        ("crates/a/Cargo.toml", "[package\nname ="),
        ("crates/a/src/lib.rs", A_LIB),
        ("crates/a/src/m.rs", A_M),
        ("crates/b/Cargo.toml", B_MANIFEST),
        ("crates/b/src/lib.rs", B_LIB),
        ("crates/b/src/deep.rs", B_DEEP),
    ]);
    let paths: Vec<_> = x.unreadable.iter().map(|(p, _)| p.as_str()).collect();
    assert_eq!(
        paths,
        [
            "crates/a/Cargo.toml",
            "crates/a/src/lib.rs",
            "crates/a/src/m.rs"
        ]
    );
    assert_eq!(items(&x).len(), 2);
}

#[test]
fn a_source_with_no_package_is_refused_by_name() {
    let x = extract(&[
        ("Cargo.toml", "[workspace]\nmembers = []\n"),
        ("src/lib.rs", "//! X.\n"),
    ]);
    assert!(items(&x).is_empty());
    assert!(
        x.unreadable.iter().any(|(_, w)| w.contains("no [package]")),
        "{:?}",
        x.unreadable
    );
}

#[test]
fn a_code_edit_below_the_doc_leaves_the_module_claim_unchanged() {
    let before = extract(&ws());
    let after = with(
        "crates/a/src/m.rs",
        "//! Module m.\n//!\n//! More about m.\n\npub fn f() { 1; }\n\nfn g() {}\n",
    );
    assert_eq!(before, after);
}

#[test]
fn adding_a_pub_fn_leaves_the_module_claim_unchanged() {
    let before = extract(&ws());
    let after = with(
        "crates/a/src/m.rs",
        &format!("{A_M}\npub fn added() {{}}\n"),
    );
    assert_eq!(
        get(&before, "rust/mod/oml_a::m"),
        get(&after, "rust/mod/oml_a::m")
    );
    assert_eq!(before, after);
}

#[test]
fn a_reflowed_module_doc_keeps_its_hash_and_a_changed_word_does_not() {
    let before = extract(&ws());
    let reflowed = with(
        "crates/a/src/m.rs",
        "//! Module m.\n//! More about\n//! m.\n\npub fn f() {}\n",
    );
    let (b, r) = (
        get(&before, "rust/mod/oml_a::m"),
        get(&reflowed, "rust/mod/oml_a::m"),
    );
    assert_eq!(b.hash, r.hash);
    let changed = with(
        "crates/a/src/m.rs",
        "//! Module m.\n//!\n//! More about n.\n\npub fn f() {}\n",
    );
    assert_ne!(b.hash, get(&changed, "rust/mod/oml_a::m").hash);
}

#[test]
fn a_version_bump_or_an_external_dependency_leaves_the_crate_claim_unchanged() {
    let before = extract(&ws());
    let after = with(
        "crates/a/Cargo.toml",
        &format!("{A_MANIFEST}\n[features]\nx = []\n")
            .replace("serde = \"1\"", "serde = \"2\"\ntokio = \"1\""),
    );
    assert_eq!(
        get(&before, "rust/crate/oml-a"),
        get(&after, "rust/crate/oml-a")
    );
}

#[test]
fn a_new_internal_dependency_changes_both_crate_claims() {
    let before = extract(&ws());
    let after = with(
        "crates/b/Cargo.toml",
        &format!("{B_MANIFEST}\n[dependencies]\noml-a.workspace = true\n"),
    );
    for key in ["rust/crate/oml-a", "rust/crate/oml-b"] {
        assert_ne!(get(&before, key).hash, get(&after, key).hash, "{key}");
    }
    assert_eq!(
        get(&before, "rust/mod/oml_a::m"),
        get(&after, "rust/mod/oml_a::m")
    );
}

#[test]
fn a_line_added_above_the_doc_keeps_the_text_and_hash_and_moves_the_span() {
    let before = extract(&ws());
    let after = with("crates/a/src/m.rs", &format!("// licence\n\n{A_M}"));
    let (b, a) = (
        get(&before, "rust/mod/oml_a::m"),
        get(&after, "rust/mod/oml_a::m"),
    );
    assert_eq!((&b.text, &b.hash), (&a.text, &a.hash));
    assert_eq!(a.fragment.as_deref(), Some("L3-L5"));
}

#[test]
fn a_renamed_module_changes_its_key_and_its_text() {
    let before = extract(&ws());
    let list: Vec<(&str, &str)> = ws()
        .into_iter()
        .map(|(p, t)| match p {
            "crates/a/src/m.rs" => ("crates/a/src/renamed.rs", t),
            "crates/a/src/lib.rs" => (p, "//! A root.\n\npub mod renamed;\nmod hidden;\n"),
            _ => (p, t),
        })
        .collect();
    let after = extract(&list);
    let keys = |x: &SetExtraction| -> Vec<String> {
        items(x).iter().map(|i| i.claim_key.clone()).collect()
    };
    assert!(keys(&before).contains(&"rust/mod/oml_a::m".to_owned()));
    assert!(!keys(&after).contains(&"rust/mod/oml_a::m".to_owned()));
    assert_ne!(
        get(&before, "rust/mod/oml_a::m").text,
        get(&after, "rust/mod/oml_a::renamed").text
    );
}

#[test]
fn the_rust_adapter_is_named_rust_and_owns_the_rust_namespace() {
    assert_eq!(Kind::parse("rust"), Ok(Kind::Rust));
    assert_eq!(Kind::Rust.name(), "rust");
    assert_eq!(Kind::of_key("rust/crate/oml-app"), Some(Kind::Rust));
    assert_eq!(Kind::of_key("rust/mod/oml_app::ports"), Some(Kind::Rust));
    assert_eq!(Kind::of_key("doc/guide/alpha"), Some(Kind::Headings));
    assert!(RustAdapter.accepts("crates/a/src/lib.rs"));
    assert!(RustAdapter.accepts("crates/a/Cargo.toml"));
    assert!(!RustAdapter.accepts("docs/guide.md"));
}

/// Every `.rs` file under a `src/` directory of this repository's crates.
fn repository_sources() -> Vec<PathBuf> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let p = entry.unwrap().path();
            if p.is_dir() {
                walk(&p, out);
            } else if p.extension().is_some_and(|e| e == "rs") {
                out.push(p);
            }
        }
    }
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../crates");
    let mut out = Vec::new();
    for manifest in glob_manifests(&crates) {
        let src = manifest.parent().unwrap().join("src");
        if src.is_dir() {
            walk(&src, &mut out);
        }
    }
    out.sort();
    out
}

fn glob_manifests(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(dir).unwrap() {
        let p = entry.unwrap().path();
        if p.is_dir() {
            if p.join("Cargo.toml").is_file() {
                out.push(p.join("Cargo.toml"));
            } else {
                out.extend(glob_manifests(&p));
            }
        }
    }
    out
}

#[test]
fn the_lexer_reads_the_same_inner_docs_as_syn_on_every_source_file() {
    let sources = repository_sources();
    assert!(sources.len() > 100, "{} files", sources.len());
    for path in sources {
        let text = std::fs::read_to_string(&path).unwrap();
        let tree = syn::parse_file(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        let expected: Vec<String> = tree
            .attrs
            .iter()
            .filter(|a| matches!(a.style, syn::AttrStyle::Inner(_)) && a.path().is_ident("doc"))
            .filter_map(|a| match &a.meta {
                syn::Meta::NameValue(nv) => match &nv.value {
                    syn::Expr::Lit(l) => match &l.lit {
                        syn::Lit::Str(s) => Some(s.value()),
                        _ => None,
                    },
                    _ => None,
                },
                _ => None,
            })
            .collect();
        let read = leading_docs(&text)
            .unwrap()
            .map(|d| d.lines)
            .unwrap_or_default();
        assert_eq!(read, expected, "{}", path.display());
    }
}

// Malformed and hostile input (PR 88 review): the lexer never panics, never
// lets code into a claim, and a file or doc over its cap is skipped by name.

#[test]
fn an_unterminated_block_doc_ending_in_a_multibyte_character_does_not_panic() {
    assert_eq!(leading_docs("/*! 日").unwrap(), None);
    assert_eq!(leading_docs("/*!é").unwrap(), None);
}

#[test]
fn an_unterminated_inner_attribute_ending_in_a_multibyte_character_does_not_panic() {
    assert_eq!(leading_docs("#![é").unwrap(), None);
    assert_eq!(leading_docs("//! kept\n#![日").unwrap(), None);
}

#[test]
fn an_unterminated_block_doc_is_no_doc_and_never_carries_the_code_after_it() {
    let src = "/*! d\nfn evil(){}\n";
    assert_eq!(leading_docs(src).unwrap(), None);
    let x = extract(&[
        ("crates/a/Cargo.toml", "[package]\nname = \"oml-a\"\n"),
        ("crates/a/src/lib.rs", "//! A root.\npub mod m;\n"),
        ("crates/a/src/m.rs", src),
    ]);
    assert!(
        items(&x).iter().all(|i| !i.text.contains("evil")),
        "{:?}",
        items(&x)
    );
    assert!(!items(&x).iter().any(|i| i.claim_key == "rust/mod/oml_a::m"));
    let warned: Vec<&String> = x
        .files
        .iter()
        .flat_map(|(_, e)| &e.warnings)
        .filter(|w| w.contains("unterminated"))
        .collect();
    assert_eq!(warned.len(), 1, "{x:?}");
}

/// A small deterministic generator, so the property test needs no crate.
struct Xorshift(u64);

impl Xorshift {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

#[test]
fn over_random_input_the_lexer_never_panics_and_no_code_reaches_a_doc() {
    // Heads are what may precede the first item; the code marker follows,
    // and the tail holds no `*/`, so nothing after the marker can close a
    // block that opened before it.
    let heads = [
        "//! d\n",
        "//!é\n",
        "/*! d */\n",
        "/*! d\n",
        "/*! 日",
        "/* c */\n",
        "// c\n",
        "#![allow(x)]\n",
        "#![é",
        "#![doc",
        "\r\n",
        " ",
        "/*",
        "/*!",
        "\u{feff}",
        "#!/bin/x\n",
    ];
    let tails = [
        "fn a() {}\n",
        "//! late\n",
        "/*! late\n",
        "é",
        "\"",
        "#![",
        "\n",
    ];
    let mut rng = Xorshift(0x9e37_79b9_7f4a_7c15);
    for _ in 0..30_000 {
        let mut src = String::new();
        for _ in 0..rng.below(6) {
            src.push_str(heads[rng.below(heads.len())]);
        }
        src.push_str("fn CODE_MARKER() {}\n");
        for _ in 0..rng.below(4) {
            src.push_str(tails[rng.below(tails.len())]);
        }
        if let Ok(Some(d)) = leading_docs(&src) {
            let text = d.lines.join("\n");
            assert!(!text.contains("CODE_MARKER"), "{src:?} gave {text:?}");
            assert!(!text.contains("late"), "{src:?} gave {text:?}");
        }
    }
    // And over any bytes at all: no panic.
    let alphabet = [
        "/", "*", "!", "#", "[", "]", "\"", "\\", "\n", "\r", " ", "é", "日", "'", "r", "b", "doc",
        "=", "//!", "/*!", "*/", "#![",
    ];
    for _ in 0..30_000 {
        let mut src = String::new();
        for _ in 0..rng.below(24) {
            src.push_str(alphabet[rng.below(alphabet.len())]);
        }
        let _ = leading_docs(&src);
        let _ = extract(&[
            ("c/Cargo.toml", "[package]\nname = \"c\"\n"),
            ("c/src/lib.rs", &src),
        ]);
    }
}

#[test]
fn a_source_file_over_the_size_cap_is_skipped_by_name_and_the_others_are_read() {
    let big = format!("//! Big.\n//! {}\n", "x".repeat(1_048_576));
    let x = extract(&[
        ("crates/a/Cargo.toml", "[package]\nname = \"oml-a\"\n"),
        ("crates/a/src/lib.rs", "//! A root.\npub mod m;\npub mod n;\n"),
        ("crates/a/src/m.rs", &big),
        ("crates/a/src/n.rs", "//! N.\n"),
    ]);
    assert!(
        x.skipped
            .iter()
            .any(|(p, why)| p == "crates/a/src/m.rs" && why.starts_with("too large")),
        "{:?}",
        x.skipped
    );
    assert!(x.unreadable.is_empty(), "{:?}", x.unreadable);
    assert!(items(&x).iter().any(|i| i.claim_key == "rust/mod/oml_a::n"));
    assert!(!items(&x).iter().any(|i| i.claim_key == "rust/mod/oml_a::m"));
}

#[test]
fn a_doc_block_over_its_cap_is_skipped_by_name_and_writes_no_claim() {
    let doc = format!("//! {}\n", "y".repeat(70_000));
    let x = extract(&[
        ("crates/a/Cargo.toml", "[package]\nname = \"oml-a\"\n"),
        ("crates/a/src/lib.rs", "//! A root.\npub mod m;\n"),
        ("crates/a/src/m.rs", &doc),
    ]);
    assert!(
        x.skipped
            .iter()
            .any(|(p, why)| p == "crates/a/src/m.rs" && why.starts_with("too large")),
        "{:?}",
        x.skipped
    );
    assert!(!items(&x).iter().any(|i| i.claim_key == "rust/mod/oml_a::m"));
    assert!(x.unreadable.is_empty());
}

#[test]
fn a_manifest_over_the_size_cap_is_unreadable_by_name() {
    let manifest = format!("[package]\nname = \"oml-a\"\n# {}\n", "z".repeat(262_144));
    let x = extract(&[
        ("crates/a/Cargo.toml", &manifest),
        ("crates/a/src/lib.rs", "//! A root.\n"),
    ]);
    assert!(
        x.unreadable
            .iter()
            .any(|(p, why)| p == "crates/a/Cargo.toml" && why.contains("too large")),
        "{:?}",
        x.unreadable
    );
    assert!(items(&x).is_empty());
}

// Workspaces (PR 88 review, M1, M2).

const WS_ROOT: &str = "[workspace]\nmembers = [\"crates/*\", \"tools/x\"]\nexclude = [\"crates/old\"]\n\n[workspace.dependencies]\nfoo = { package = \"oml-b\", path = \"crates/b\" }\n";

fn member(name: &str, extra: &str) -> String {
    format!("[package]\nname = \"{name}\"\n{extra}")
}

#[test]
fn only_the_packages_a_workspace_lists_are_crates() {
    let (a, b, old, tool, ex) = (
        member("oml-a", ""),
        member("oml-b", ""),
        member("oml-old", ""),
        member("oml-x", ""),
        member("oml-example", ""),
    );
    let x = extract(&[
        ("Cargo.toml", WS_ROOT),
        ("crates/a/Cargo.toml", &a),
        ("crates/a/src/lib.rs", "//! A.\n"),
        ("crates/b/Cargo.toml", &b),
        ("crates/b/src/lib.rs", "//! B.\n"),
        ("crates/old/Cargo.toml", &old),
        ("crates/old/src/lib.rs", "//! Old.\n"),
        ("tools/x/Cargo.toml", &tool),
        ("tools/x/src/lib.rs", "//! X.\n"),
        ("examples/e/Cargo.toml", &ex),
        ("examples/e/src/lib.rs", "//! E.\n"),
    ]);
    let mut keys: Vec<&str> = items(&x).iter().map(|i| i.claim_key.as_str()).collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        ["rust/crate/oml-a", "rust/crate/oml-b", "rust/crate/oml-x"]
    );
    for gone in ["crates/old/Cargo.toml", "examples/e/Cargo.toml"] {
        assert!(
            x.skipped
                .iter()
                .any(|(p, why)| p == gone && why.starts_with("not a workspace member")),
            "{gone}: {:?}",
            x.skipped
        );
    }
}

#[test]
fn a_path_dependency_inside_the_workspace_is_a_member_without_being_listed() {
    let root = "[workspace]\nmembers = [\"crates/a\"]\n";
    let a = member(
        "oml-a",
        "\n[dependencies]\nhelper = { path = \"../helper\" }\n",
    );
    let h = member("helper", "");
    let x = extract(&[
        ("Cargo.toml", root),
        ("crates/a/Cargo.toml", &a),
        ("crates/a/src/lib.rs", "//! A.\n"),
        ("crates/helper/Cargo.toml", &h),
        ("crates/helper/src/lib.rs", "//! H.\n"),
    ]);
    assert!(items(&x).iter().any(|i| i.claim_key == "rust/crate/helper"));
}

#[test]
fn a_dependency_renamed_through_the_workspace_table_names_its_real_package() {
    let a = member("oml-a", "\n[dependencies]\nfoo.workspace = true\n");
    let b = member("oml-b", "");
    let x = extract(&[
        ("Cargo.toml", WS_ROOT),
        ("crates/a/Cargo.toml", &a),
        ("crates/a/src/lib.rs", "//! A.\n"),
        ("crates/b/Cargo.toml", &b),
        ("crates/b/src/lib.rs", "//! B.\n"),
    ]);
    let text = &get(&x, "rust/crate/oml-a").text;
    assert!(
        text.contains("Depends on (in this workspace): oml-b."),
        "{text}"
    );
    assert!(
        get(&x, "rust/crate/oml-b")
            .text
            .contains("Depended on by (in this workspace): oml-a.")
    );
}

#[test]
fn a_dependency_renamed_with_package_names_its_real_package() {
    let a = member(
        "oml-a",
        "\n[dependencies]\nbee = { package = \"oml-b\", path = \"../b\" }\n",
    );
    let b = member("oml-b", "");
    let x = extract(&[
        ("crates/a/Cargo.toml", &a),
        ("crates/a/src/lib.rs", "//! A.\n"),
        ("crates/b/Cargo.toml", &b),
        ("crates/b/src/lib.rs", "//! B.\n"),
    ]);
    assert!(
        get(&x, "rust/crate/oml-a")
            .text
            .contains("Depends on (in this workspace): oml-b.")
    );
}
