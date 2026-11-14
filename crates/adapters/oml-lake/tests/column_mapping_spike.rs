//! The lake spike of ADR-0091 d7: may the pinned deltalake rename
//! `bronze.events.workarea_id` and `silver.claims.workarea_id` to
//! `workspace_id` by Delta column mapping in `name` mode? The lake's tables
//! already exist without column mapping, and a rename by column mapping
//! first puts the table into that mode. The pinned deltalake refuses to, so
//! the decided fallback holds: the physical name stays `workarea_id`, the
//! schema page names the column `workspace_id`, and the readers map it.

use std::path::PathBuf;

use deltalake::DeltaTable;
use deltalake::errors::DeltaTableError;
use deltalake::kernel::{DataType, PrimitiveType, StructField};

#[tokio::test]
async fn the_pinned_deltalake_refuses_to_put_an_existing_table_into_column_mapping_mode() {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("column-mapping-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("dir");
    let url = url::Url::from_directory_path(&dir).expect("url");
    let table = DeltaTable::try_from_url(url)
        .await
        .expect("table")
        .create()
        .with_columns([StructField::new(
            "workarea_id",
            DataType::Primitive(PrimitiveType::String),
            true,
        )])
        .await
        .expect("an existing lake table, created without column mapping");
    let err = table
        .set_tbl_properties()
        .with_properties([("delta.columnMapping.mode".to_owned(), "name".to_owned())].into())
        .await
        .expect_err("the pinned deltalake does not enable column mapping on an existing table");
    assert!(
        matches!(err, DeltaTableError::UnsupportedColumnMapping { .. }),
        "{err}"
    );
    std::fs::remove_dir_all(&dir).expect("clean");
}
