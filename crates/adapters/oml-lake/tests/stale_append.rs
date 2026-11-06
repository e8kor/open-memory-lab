//! An append cut against an older transaction version is refused (ADR-0060
//! d3): a writer that read the version, lost a race to another writer, and
//! then loaded a fresh snapshot must not append the rows the winner holds.

use std::path::PathBuf;

use oml_lake::table::{LAKE_BATCHES, V};
use oml_lake::{Lake, LakeError, Prefix};

fn row(start: i64, end: i64) -> Vec<V> {
    vec![
        V::Long(Some(start)),
        V::Long(Some(end)),
        V::Long(Some(1)),
        V::Long(Some(end - start)),
        V::Ts(Some(0)),
    ]
}

#[tokio::test]
async fn an_append_cut_against_an_older_transaction_version_is_refused_and_the_rows_land_once() {
    let dir =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("stale-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("dir");
    let lake = Lake::at(dir.to_str().expect("utf-8")).expect("lake");
    let p = Prefix::Provider;

    // Two writers both read version 0 and cut the same rows.
    let (a, b) = (row(0, 10), row(0, 10));
    assert_eq!(
        lake.append(p, &LAKE_BATCHES, &[a], 10, 0)
            .await
            .expect("first"),
        1
    );
    let err = lake
        .append(p, &LAKE_BATCHES, &[b], 10, 0)
        .await
        .expect_err("the second was cut against version 0");
    assert!(
        matches!(
            err,
            LakeError::Stale {
                filtered_at: 0,
                now: 10,
                ..
            }
        ),
        "{err}"
    );
    assert_eq!(lake.applied(p, &LAKE_BATCHES).await.expect("applied"), 10);

    // Re-cut against the version the table holds, it lands.
    assert_eq!(
        lake.append(p, &LAKE_BATCHES, &[row(10, 20)], 20, 10)
            .await
            .expect("next"),
        1
    );
    assert_eq!(lake.applied(p, &LAKE_BATCHES).await.expect("applied"), 20);
    let _ = std::fs::remove_dir_all(&dir);
}
