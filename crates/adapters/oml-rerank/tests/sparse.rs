//! The learned sparse encoder against the PyTorch reference (ADR-0052 d14).
//!
//! `tests/fixtures/doc-v3-distill.json` is what
//! `eval/bench/offline/sparse_fixture.py` wrote with the offline
//! experiment's encoder: per document its input ids and every non-zero
//! weight, per query its ids and idf weights. The model's files are not in
//! the repository; the tests that need them are ignored by default and run
//! with `--ignored` where `OML_SPARSE_MODEL_DIR` names a directory holding
//! them, as CI's model step does from its cache.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use oml_app::ports::SparseEncoder;
use oml_rerank::{
    DOC_V3_DISTILL, LoadError, SPARSE_FILES, SPARSE_FLOOR, SPARSE_MAX_TOKENS, SparseModel,
    check_sparse_files,
};

/// The tolerance each weight is held to: the experiment's report asked
/// for 1e-4 (REPORT.md, "What moves needs more to serve", 2).
const TOLERANCE: f32 = 1e-4;

fn model_dir() -> PathBuf {
    PathBuf::from(
        std::env::var_os("OML_SPARSE_MODEL_DIR")
            .unwrap_or_else(|| panic!("run with --ignored only where OML_SPARSE_MODEL_DIR is set")),
    )
}

fn fixture() -> serde_json::Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/doc-v3-distill.json");
    serde_json::from_slice(&std::fs::read(path).expect("fixture")).expect("fixture parses")
}

fn pairs(v: &serde_json::Value) -> BTreeMap<u32, f32> {
    v.as_array()
        .expect("pairs")
        .iter()
        .map(|p| {
            #[allow(clippy::cast_possible_truncation, reason = "ids and weights fit")]
            (
                p[0].as_u64().expect("id") as u32,
                p[1].as_f64().expect("weight") as f32,
            )
        })
        .collect()
}

#[test]
#[ignore = "needs the model's files in OML_SPARSE_MODEL_DIR"]
fn the_sparse_encoder_matches_the_python_reference() {
    let model =
        SparseModel::load(&model_dir(), &DOC_V3_DISTILL, 2).expect("the pinned model loads");
    let f = fixture();
    assert_eq!(f["revision"], DOC_V3_DISTILL.revision);
    let mut worst = 0.0_f32;
    for doc in f["docs"].as_array().expect("docs") {
        let text = doc["text"].as_str().expect("text");
        let ids: Vec<u32> = doc["input_ids"]
            .as_array()
            .expect("ids")
            .iter()
            .map(|i| u32::try_from(i.as_u64().expect("id")).expect("u32"))
            .collect();
        assert_eq!(
            model.input_ids(text).expect("ids"),
            ids,
            "input ids of {text:.40}"
        );
        let want = pairs(&doc["weights"]);
        let got: BTreeMap<u32, f32> = model
            .raw_weights(text)
            .expect("weights")
            .into_iter()
            .collect();
        for id in want.keys().chain(got.keys()) {
            let (w, g) = (
                want.get(id).copied().unwrap_or(0.0),
                got.get(id).copied().unwrap_or(0.0),
            );
            worst = worst.max((w - g).abs());
            assert!(
                (w - g).abs() <= TOLERANCE,
                "token {id} of {text:.40}: reference {w}, candle {g}"
            );
        }
    }
    eprintln!("largest weight difference: {worst:e}");
    for q in f["queries"].as_array().expect("queries") {
        let text = q["text"].as_str().expect("text");
        let want = pairs(&q["weights"]);
        let got: BTreeMap<u32, f32> = model
            .encode_query(text)
            .into_iter()
            .map(|t| (t.token_id, t.weight))
            .collect();
        assert_eq!(
            got.keys().collect::<Vec<_>>(),
            want.keys().collect::<Vec<_>>(),
            "query ids of {text}"
        );
        for (id, w) in &want {
            assert!((got[id] - w).abs() <= 1e-6, "query {text} token {id}");
        }
    }
}

#[test]
#[ignore = "needs the model's files in OML_SPARSE_MODEL_DIR"]
fn a_document_keeps_its_largest_weights_above_the_floor_and_names_its_expansions() {
    let model = SparseModel::load(&model_dir(), &DOC_V3_DISTILL, 2).expect("loads");
    let text = "We deploy the api service with a canary that takes 5% of traffic for 20 minutes.";
    let postings = model.document(text).expect("document");
    assert!(!postings.is_empty() && postings.len() <= SPARSE_MAX_TOKENS);
    assert!(postings.iter().all(|p| p.weight >= SPARSE_FLOOR));
    let raw = model.raw_weights(text).expect("raw");
    let above = raw.iter().filter(|(_, w)| *w >= SPARSE_FLOOR).count();
    assert_eq!(postings.len(), above.min(SPARSE_MAX_TOKENS));
    let canary = model.input_ids("canary").expect("ids")[1];
    assert!(
        postings.iter().any(|p| p.token_id == canary && p.in_text),
        "a word the text says is not an expansion"
    );
    assert!(
        postings.iter().any(|p| !p.in_text),
        "the encoder adds tokens the text does not hold"
    );
}

#[test]
fn a_missing_or_altered_sparse_file_is_refused_by_name() {
    let dir = std::env::temp_dir().join(format!("oml-sparse-check-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("dir");
    let missing = check_sparse_files(&dir, &DOC_V3_DISTILL).unwrap_err();
    assert!(
        matches!(
            missing,
            LoadError::Missing {
                file: "config.json",
                ..
            }
        ),
        "{missing}"
    );
    std::fs::write(dir.join(SPARSE_FILES[0]), b"{}").expect("write");
    let altered = check_sparse_files(&dir, &DOC_V3_DISTILL).unwrap_err();
    assert!(
        matches!(
            altered,
            LoadError::Digest {
                file: "config.json",
                ..
            }
        ),
        "{altered}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_pinned_model_is_apache_licensed_at_a_fixed_revision() {
    assert_eq!(DOC_V3_DISTILL.licence, "Apache-2.0");
    assert_eq!(DOC_V3_DISTILL.revision.len(), 40);
    assert!(
        DOC_V3_DISTILL
            .url("idf.json")
            .contains(DOC_V3_DISTILL.revision)
    );
}
