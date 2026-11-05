#!/usr/bin/env bash
# One end-to-end benchmark run: boot a lead (boot.sh), load each dataset,
# run every pipeline over it, print the tables, and stop the lead, which drops
# its database. The nightly workflow (.github/workflows/bench.yml) runs it;
# so can anyone with a PostgreSQL superuser:
#
#   PGHOST=127.0.0.1 PGPORT=55440 PGUSER=postgres OML_BIN_DIR=target/release \
#       eval/bench/run.sh <out-dir> synthetic scifact
#
# BENCH_PYTHON names an interpreter with ir_measures and ir_datasets
# (eval/bench/requirements.txt); BENCH_CACHE the dataset cache (default
# /srv/build/bench-data). Results: <out-dir>/<dataset>_<pipeline>.json and
# <out-dir>/tables.md, with the offline bm25s reference beside each served
# pipeline (reference.py). eval/bench/export.py turns them into the
# committed oml-bench/1 files.
set -u -o pipefail
cd "$(dirname "$0")/../.."

OUT="${1:?usage: run.sh <out-dir> <dataset>...}"
shift
[ "$#" -ge 1 ] || { echo "run.sh: name at least one dataset" >&2; exit 2; }
PY="${BENCH_PYTHON:-python3}"
CACHE="${BENCH_CACHE:-/srv/build/bench-data}"
mkdir -p "$OUT"
STATE="$(mktemp -d "$OUT/state.XXXXXX")"
BOOT_PID=""
cleanup() {
    if [ -n "$BOOT_PID" ] && kill -0 "$BOOT_PID" 2>/dev/null; then
        kill -TERM "$BOOT_PID"
        wait "$BOOT_PID" 2>/dev/null
    fi
    rm -rf "$STATE"
}
trap cleanup EXIT

WORKAREAS="$("$PY" eval/bench/bench.py --cache "$CACHE" workareas "$@")" || exit 1
# shellcheck disable=SC2086 # one workarea per word
eval/bench/boot.sh "$STATE" $WORKAREAS 2>"$OUT/boot.stderr" &
BOOT_PID=$!
for _ in $(seq 1 600); do
    [ -s "$STATE/url" ] && break
    kill -0 "$BOOT_PID" 2>/dev/null || { cat "$OUT/boot.stderr" >&2; exit 1; }
    sleep 0.2
done
[ -s "$STATE/url" ] || { echo "run.sh: the lead never came up" >&2; exit 1; }

for ds in "$@"; do
    "$PY" eval/bench/bench.py --cache "$CACHE" load --state "$STATE" "$ds" || exit 1
    cp "$STATE/load-$ds.json" "$OUT/"
    # A sparse pipeline reads what the encoder wrote: wait for its queue
    # to drain, and keep how long it took (ADR-0052 d15).
    case " ${BENCH_PIPELINES:-} " in *" sparse "*|*" bm25-sparse "*)
        eval/bench/wait_sparse.sh "$(cat "$STATE/metrics_url")" >"$OUT/encode-wait-$ds.txt" || exit 1 ;;
    esac
    "$PY" eval/bench/bench.py --cache "$CACHE" run --state "$STATE" --out "$OUT" "$ds" || exit 1
done
"$PY" eval/bench/reference.py --cache "$CACHE" "$OUT" "$@" || exit 1
"$PY" eval/bench/bench.py report "$OUT" | tee "$OUT/tables.md"
