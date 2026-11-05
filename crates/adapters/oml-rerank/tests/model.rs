//! The cross-encoder adapter against a real model (ADR-0053 d10).
//!
//! The model's files are not in the repository. The tests that need them are
//! ignored by default, each naming why, and run with `--ignored` where
//! `OML_RERANK_MODEL_DIR` names a directory holding them, as CI's rerank
//! step does from its cache; the file checks and the fetch need no model.

use std::path::{Path, PathBuf};
use std::time::Duration;

use oml_app::ports::{RerankError, Reranker};
use oml_rerank::{CrossEncoder, Limits, LoadError, MINILM_L6, MODEL_FILES, fetch_missing};

const QUERY: &str = "which port does postgres listen on";

/// Why a test is ignored by default.
const NEEDS_MODEL: &str = "needs the model's files in OML_RERANK_MODEL_DIR";

/// The model directory an ignored test was asked to run with.
fn model_dir() -> PathBuf {
    PathBuf::from(
        std::env::var_os("OML_RERANK_MODEL_DIR")
            .unwrap_or_else(|| panic!("run with --ignored only where {NEEDS_MODEL}")),
    )
}

fn limits() -> Limits {
    Limits {
        max_concurrent: 2,
        threads: 2,
        max_tokens: oml_rerank::MAX_TOKENS,
        timeout: Duration::from_secs(60),
    }
}

fn model() -> CrossEncoder {
    let _ = NEEDS_MODEL;
    CrossEncoder::load(&model_dir(), &MINILM_L6, limits()).expect("the pinned model loads")
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
#[ignore = "needs the model's files in OML_RERANK_MODEL_DIR"]
async fn the_cross_encoder_passes_the_reranker_laws() {
    oml_port_contracts::run_reranker_suite(&model()).await;
}

/// The relevances the model's own ONNX export gives under onnxruntime
/// 1.31.1 for the same pairs (`eval/bench/rerank_probe.py`): the logistic of
/// the logits 9.6974, -11.4313 and 1.1022.
#[test]
#[ignore = "needs the model's files in OML_RERANK_MODEL_DIR"]
fn the_cross_encoder_scores_as_the_models_onnx_export_does() {
    let scores = model().score_now(QUERY, &passages()).expect("scores");
    for (got, want) in scores.iter().zip([0.999_938_6_f32, 0.000_010_8, 0.750_667]) {
        assert!((got - want).abs() < 1e-3, "{scores:?}");
    }
}

#[tokio::test]
#[ignore = "needs the model's files in OML_RERANK_MODEL_DIR"]
async fn a_call_past_its_deadline_answers_timeout_or_busy_never_a_late_order() {
    let model = CrossEncoder::load(
        &model_dir(),
        &MINILM_L6,
        Limits {
            max_concurrent: 1,
            timeout: Duration::from_millis(1),
            ..limits()
        },
    )
    .expect("the pinned model loads");
    let long: Vec<String> = (0..8).map(|i| format!("{} {i}", passages()[0])).collect();
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

/// A megabyte passage costs what its first `MAX_PASSAGE_BYTES` cost: it is
/// cut before the tokenizer reads it, so one author's long memory cannot
/// push every reader's rerank past its deadline (ADR-0053 d2).
#[tokio::test]
#[ignore = "needs the model's files in OML_RERANK_MODEL_DIR"]
async fn a_megabyte_passage_scores_inside_a_one_second_deadline() {
    let model = CrossEncoder::load(
        &model_dir(),
        &MINILM_L6,
        Limits {
            timeout: Duration::from_secs(1),
            ..limits()
        },
    )
    .expect("the pinned model loads");
    let huge = "postgres listens on port 5432 ".repeat(35_000);
    assert!(huge.len() > 1_000_000);
    let scores = model
        .score(QUERY.to_owned(), vec![huge.clone(), huge])
        .await
        .expect("scored inside the deadline");
    assert_eq!(scores.len(), 2);
}

/// A query longer than the model reads is cut to its share of the pair, so
/// every passage keeps tokens and the rerank still answers (ADR-0053 d2).
#[test]
#[ignore = "needs the model's files in OML_RERANK_MODEL_DIR"]
fn a_two_thousand_word_query_still_scores_every_passage_with_room_for_it() {
    let query = format!(
        "{QUERY} {}",
        "the stack trace says connection refused at line ".repeat(250)
    );
    let scores = model().score_now(&query, &passages()).expect("scored");
    assert_eq!(scores.len(), 3);
    assert!(
        scores[0] > scores[1],
        "the answer still outranks the cafeteria: {scores:?}"
    );
}

/// A passage is cut on a character boundary at its byte bound, never inside
/// a character.
#[test]
fn a_passage_is_cut_to_its_byte_bound_on_a_character_boundary() {
    let text = "é".repeat(oml_rerank::MAX_PASSAGE_BYTES);
    let cut = oml_rerank::cut_passage(&text);
    assert!(cut.len() <= oml_rerank::MAX_PASSAGE_BYTES);
    assert!(cut.len() >= oml_rerank::MAX_PASSAGE_BYTES - 1);
    assert!(text.starts_with(cut));
    assert_eq!(oml_rerank::cut_passage("short"), "short");
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
