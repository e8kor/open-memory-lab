//! The cross-encoder adapter against a real model (ADR-0053 d10).
//!
//! The model's files are not in the repository. These tests read them from
//! the directory `OML_RERANK_MODEL_DIR` names, which CI may restore from its
//! cache, and each one that needs them says it was skipped, and why, when
//! the variable is unset; the file checks and the fetch need no model.

use std::path::{Path, PathBuf};
use std::time::Duration;

use oml_app::ports::{RerankError, Reranker};
use oml_rerank::{CrossEncoder, Limits, LoadError, MINILM_L6, MODEL_FILES, fetch_missing};

const QUERY: &str = "which port does postgres listen on";

/// The model directory, or `None` with the reason printed.
fn model_dir(test: &str) -> Option<PathBuf> {
    match std::env::var_os("OML_RERANK_MODEL_DIR") {
        Some(dir) => Some(PathBuf::from(dir)),
        None => {
            eprintln!(
                "skipped {test}: OML_RERANK_MODEL_DIR is unset, so no {} files to load",
                MINILM_L6.name
            );
            None
        }
    }
}

fn limits() -> Limits {
    Limits {
        max_concurrent: 2,
        timeout: Duration::from_secs(60),
    }
}

fn passages() -> Vec<String> {
    [
        "PostgreSQL listens on port 5432 by default; the eval cluster uses 55437.",
        "The cafeteria serves lunch from noon until two on weekdays.",
        "Postgres connections go through pgbouncer in transaction mode.",
    ]
    .map(ToOwned::to_owned)
    .to_vec()
}

#[tokio::test]
async fn the_cross_encoder_passes_the_reranker_laws() {
    let Some(dir) = model_dir("the_cross_encoder_passes_the_reranker_laws") else {
        return;
    };
    let model = CrossEncoder::load(&dir, &MINILM_L6, limits()).expect("the pinned model loads");
    oml_port_contracts::run_reranker_suite(&model).await;
}

/// The relevances the model's own ONNX export gives under onnxruntime
/// 1.31.1 for the same pairs (`eval/bench/rerank_probe.py`): the logistic of
/// the logits 9.6974, -11.4313 and 1.1022.
#[test]
fn the_cross_encoder_scores_as_the_models_onnx_export_does() {
    let Some(dir) = model_dir("the_cross_encoder_scores_as_the_models_onnx_export_does") else {
        return;
    };
    let model = CrossEncoder::load(&dir, &MINILM_L6, limits()).expect("the pinned model loads");
    let scores = model.score_now(QUERY, &passages()).expect("scores");
    for (got, want) in scores.iter().zip([0.999_938_6_f32, 0.000_010_8, 0.750_667]) {
        assert!((got - want).abs() < 1e-3, "{scores:?}");
    }
}

#[tokio::test]
async fn a_call_past_its_deadline_answers_timeout_or_busy_never_a_late_order() {
    let Some(dir) =
        model_dir("a_call_past_its_deadline_answers_timeout_or_busy_never_a_late_order")
    else {
        return;
    };
    let model = CrossEncoder::load(
        &dir,
        &MINILM_L6,
        Limits {
            max_concurrent: 1,
            timeout: Duration::from_millis(1),
        },
    )
    .expect("the pinned model loads");
    let long: Vec<String> = (0..64).map(|i| format!("{} {i}", passages()[0])).collect();
    let (a, b) = tokio::join!(
        model.score(QUERY.to_owned(), long.clone()),
        model.score(QUERY.to_owned(), long)
    );
    for r in [a, b] {
        assert!(
            matches!(r, Err(RerankError::Timeout | RerankError::Busy)),
            "{r:?}"
        );
    }
}

/// A scratch directory under the target directory, emptied first.
fn scratch(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch dir");
    dir
}

#[test]
fn a_missing_file_is_refused_naming_it_and_the_remedy() {
    let dir = scratch("missing");
    let err = CrossEncoder::load(&dir, &MINILM_L6, limits()).unwrap_err();
    assert!(
        matches!(
            err,
            LoadError::Missing {
                file: "config.json",
                ..
            }
        ),
        "{err}"
    );
    assert!(err.to_string().contains("search.rerank.fetch"), "{err}");
}

#[test]
fn a_file_that_is_not_the_pinned_one_is_refused_before_it_is_parsed() {
    let dir = scratch("tampered");
    std::fs::write(dir.join("config.json"), b"{}").expect("write");
    let err = CrossEncoder::load(&dir, &MINILM_L6, limits()).unwrap_err();
    assert!(
        matches!(
            err,
            LoadError::Digest {
                file: "config.json",
                ..
            }
        ),
        "{err}"
    );
}

#[test]
fn a_fetched_file_that_is_not_the_pinned_one_is_never_written() {
    let dir = scratch("fetch-wrong");
    let err = fetch_missing(&dir, &MINILM_L6, |_| Ok(b"not the model".to_vec())).unwrap_err();
    assert!(matches!(err, LoadError::Digest { .. }), "{err}");
    for file in MODEL_FILES {
        assert!(!dir.join(file).exists(), "{file} was written");
        assert!(!dir.join(format!("{file}.partial")).exists());
    }
}

#[test]
fn fetch_asks_only_for_the_missing_files_at_the_pinned_revision() {
    let dir = scratch("fetch-urls");
    std::fs::write(dir.join("config.json"), b"placed by the operator").expect("write");
    let asked = std::sync::Mutex::new(Vec::new());
    let _ = fetch_missing(&dir, &MINILM_L6, |url| {
        asked.lock().expect("lock").push(url.to_owned());
        Err("offline".to_owned())
    });
    let asked = asked.into_inner().expect("lock");
    assert_eq!(
        asked,
        vec![format!(
            "https://huggingface.co/{}/resolve/{}/tokenizer.json",
            MINILM_L6.repo, MINILM_L6.revision
        )],
        "the placed file is not fetched, and a failed fetch stops the rest"
    );
}
