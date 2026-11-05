#!/usr/bin/env bash
# Waits until the lead's learned sparse encoder has drained its queue
# (ADR-0052 d15): `oml_sparse_backlog` on the metrics listener at <url> reads
# 0 twice, 2 s apart, so a sparse run never measures a half-encoded corpus.
# Prints the wait in seconds; gives up after BENCH_SPARSE_WAIT seconds
# (default 7200).
#
#   eval/bench/wait_sparse.sh http://127.0.0.1:9447
set -u -o pipefail
URL="${1:?usage: wait_sparse.sh <metrics-url>}"
LIMIT="${BENCH_SPARSE_WAIT:-7200}"
start=$(date +%s)
zero=0
while :; do
    n="$(curl -fsS "$URL/metrics" 2>/dev/null | awk '/^oml_sparse_backlog /{print $2}')"
    if [ "${n:-x}" = "0" ]; then
        zero=$((zero + 1))
        [ "$zero" -ge 2 ] && break
    else
        zero=0
    fi
    [ $(( $(date +%s) - start )) -lt "$LIMIT" ] || { echo "wait_sparse.sh: backlog still ${n:-unknown} after ${LIMIT}s" >&2; exit 1; }
    sleep 2
done
echo $(( $(date +%s) - start ))
