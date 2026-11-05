#!/usr/bin/env bash
# Waits until the lead's learned sparse encoder has written at least
# <written> versions in all (oml_sparse_encoded_total{outcome="written"}) and
# its queue reads empty (oml_sparse_backlog 0), on the metrics listener at
# <url> (ADR-0052 d15), so a sparse run never measures a half-encoded
# corpus: an idle encoder reads the gauge only every half minute, so a zero
# alone may predate the load. Prints the wait in seconds; gives up after
# BENCH_SPARSE_WAIT seconds (default 7200).
#
#   eval/bench/wait_sparse.sh http://127.0.0.1:9447 5882
set -u -o pipefail
URL="${1:?usage: wait_sparse.sh <metrics-url> <written>}"
WANT="${2:?usage: wait_sparse.sh <metrics-url> <written>}"
LIMIT="${BENCH_SPARSE_WAIT:-7200}"
start=$(date +%s)
while :; do
    text="$(curl -fsS "$URL/metrics" 2>/dev/null || true)"
    n="$(awk '/^oml_sparse_backlog /{print $2}' <<<"$text")"
    w="$(awk '/^oml_sparse_encoded_total\{outcome="written"\}/{print int($2)}' <<<"$text")"
    if [ "${n:-x}" = "0" ] && [ "${w:-0}" -ge "$WANT" ]; then
        break
    fi
    [ $(( $(date +%s) - start )) -lt "$LIMIT" ] || { echo "wait_sparse.sh: written ${w:-0} of $WANT, backlog ${n:-unknown} after ${LIMIT}s" >&2; exit 1; }
    sleep 2
done
echo $(( $(date +%s) - start ))
