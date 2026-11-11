#!/usr/bin/env bash
# The served recall eval in one command, as CI's recall-eval job runs it
# (ADR-0049 d9): builds the lead and the CLI from this tree, fetches the
# reranker's and the sparse encoder's pinned files when the pipelines need
# them, then runs served.sh, which boots the lead against a throwaway
# database, loads the set, runs every pipeline through oml_read and gates the
# results. Needs a PostgreSQL superuser on a throwaway cluster, such as the
# compose database (docker compose up -d postgres):
#
#   PGPORT=5432 eval/recall/selfhost.sh [out-dir]       # default .tmp/recall-eval
#
# EVAL_PIPELINES narrows the run (default: every pipeline CI measures).
# OML_BIN_DIR names prebuilt binaries and skips the build. EVAL_MODELS is
# where the model files go (default .tmp/oml-models); a file already there is
# checked against its pinned digest, never fetched again. The rest of
# served.sh's settings (PGHOST, PGUSER, PGPASSWORD, EVAL_DEFAULT, EVAL_PORT)
# pass through. docs/operations/recall-eval.md is the page.
set -euo pipefail
cd "$(dirname "$0")/../.."

OUT="${1:-.tmp/recall-eval}"
export EVAL_PIPELINES="${EVAL_PIPELINES:-keyword-all keyword-any bm25 bm25-trigram bm25-postings bm25-rerank sparse bm25-sparse}"
MODELS="${EVAL_MODELS:-.tmp/oml-models}"
mkdir -p "$MODELS"
MODELS="$(cd "$MODELS" && pwd)"

need() {
    command -v "$1" >/dev/null 2>&1 || { echo "selfhost.sh: $1 is required: $2" >&2; exit 1; }
}
need psql "it creates and drops the throwaway database and its role pair"
need jq "it reads the keys the CLI issues"
need curl "it waits for the lead and fetches the model files"
need python3 "run.py and gate.py run on the standard library"
need sha256sum "it checks each model file against its pinned digest"
if [ -z "${PGPORT:-}" ]; then
    echo "selfhost.sh: set PGPORT to a throwaway PostgreSQL cluster whose PGUSER is a superuser, such as the compose database (docker compose up -d postgres, port 5432)" >&2
    exit 1
fi

if [ -z "${OML_BIN_DIR:-}" ]; then
    need cargo "it builds oml-server-lead and oml from this tree"
    cargo build --release --locked --bin oml-server-lead --bin oml
    export OML_BIN_DIR="${CARGO_TARGET_DIR:-target}/release"
fi

case " $EVAL_PIPELINES " in
*" bm25-rerank "*)
    export OML_SEARCH__RERANK__MODEL_DIR="${OML_SEARCH__RERANK__MODEL_DIR:-$MODELS/ms-marco-MiniLM-L6-v2}"
    bash scripts/fetch_rerank_model.sh "$OML_SEARCH__RERANK__MODEL_DIR"
    # bm25-rerank is gated on quality; its deadline is raised so a slow
    # machine measures the ranking, not its own CPU (ADR-0053 d9).
    export OML_SEARCH__RERANK__TIMEOUT_MS="${OML_SEARCH__RERANK__TIMEOUT_MS:-60000}"
    ;;
esac
case " $EVAL_PIPELINES " in
*" sparse "* | *" bm25-sparse "* | *" bm25-sparse-rrf "*)
    export OML_SEARCH__SPARSE__MODEL_DIR="${OML_SEARCH__SPARSE__MODEL_DIR:-$MODELS/doc-v3-distill}"
    bash scripts/fetch_sparse_model.sh "$OML_SEARCH__SPARSE__MODEL_DIR"
    ;;
esac

exec bash eval/recall/served.sh "$OUT"
