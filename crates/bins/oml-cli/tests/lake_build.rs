//! The default `oml` and the lake build (ADR-0062 d3): the lake reader is a
//! cargo feature, so the default CLI's dependency tree is the one it had
//! before the feature existed, and the default CLI refuses the `delta` adapter
//! naming the build that has it.

use std::collections::BTreeSet;
use std::process::Command;

/// `cargo tree` of this package's normal and build dependencies on every
/// target: one `name version` per package, sorted, without any path.
fn tree(features: &[&str]) -> BTreeSet<String> {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let mut cmd = Command::new(cargo);
    cmd.current_dir(env!("CARGO_MANIFEST_DIR")).args([
        "tree",
        "--color",
        "never",
        "--locked",
        "-p",
        "oml-cli",
        "-e",
        "normal,build",
        "--target",
        "all",
        "--prefix",
        "none",
        "--format",
        "{p}",
    ]);
    if !features.is_empty() {
        cmd.args(["--features", &features.join(",")]);
    }
    let out = cmd.output().expect("cargo tree runs");
    assert!(
        out.status.success(),
        "cargo tree failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout)
        .unwrap()
        .lines()
        .map(|l| l.split(" (").next().unwrap().to_owned())
        .collect()
}

#[test]
fn the_default_cli_dependency_tree_is_unchanged() {
    let golden: BTreeSet<String> = include_str!("fixtures/default-tree.txt")
        .lines()
        .map(str::to_owned)
        .collect();
    let now = tree(&[]);
    assert_eq!(
        now.difference(&golden).collect::<Vec<_>>(),
        Vec::<&String>::new(),
        "packages the default CLI gained"
    );
    assert_eq!(
        golden.difference(&now).collect::<Vec<_>>(),
        Vec::<&String>::new(),
        "packages the default CLI lost"
    );
    for name in ["parquet ", "oml-delta-log "] {
        assert!(!now.iter().any(|p| p.starts_with(name)), "{name} is linked");
    }
}

#[test]
fn the_lake_build_adds_the_reader_and_parquet_and_no_arrow() {
    let lake = tree(&["lake-sync"]);
    for name in ["oml-delta-log ", "parquet "] {
        assert!(lake.iter().any(|p| p.starts_with(name)), "{name} is absent");
    }
    for banned in ["arrow", "deltalake", "object_store", "tokio "] {
        assert!(
            !lake.iter().any(|p| p.starts_with(banned)),
            "{banned} is linked into the lake build"
        );
    }
}

#[cfg(not(feature = "lake-sync"))]
#[test]
fn the_default_cli_refuses_the_delta_adapter_naming_the_lake_build() {
    let out = Command::new(env!("CARGO_BIN_EXE_oml"))
        .args(["sync", ".", "--adapter", "delta", "--workarea", "w"])
        .env_remove("OML_API_KEY")
        .env_remove("OML_URL")
        .env("HOME", env!("CARGO_MANIFEST_DIR"))
        .output()
        .unwrap();
    assert!(!out.status.success());
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(err.contains("oml-lake"), "{err}");
    assert!(err.contains("delta"), "{err}");
}
