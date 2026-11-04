#!/bin/sh
# The full sequence, one run at a time (runs share the host; parallel runs
# would measure each other). Logs go beside the cluster, results to results/.
set -e
cd "$(dirname "$0")"
PY=${PY:-/srv/build/eval-venv/bin/python}
LOG=${LOG:-/srv/build/pg-scale}
$PY run.py --scale 10000 --out results/s10k.json > $LOG/s10k.log 2>&1
$PY run.py --scale 100000 --out results/s100k.json > $LOG/s100k.log 2>&1
$PY run.py --scale 100000 --tenants 10 --skip vector --out results/mt10x100k.json > $LOG/mt.log 2>&1
$PY run.py --scale 1000000 --skip vector --timeout-ms 5000 --norls-one-pass --out results/s1m_text.json > $LOG/s1m_text.log 2>&1
$PY run.py --scale 1000000 --skip bm25,dual,trgm --timeout-ms 5000 --norls-one-pass --out results/s1m_vector.json > $LOG/s1m_vector.log 2>&1
# Supplement: the stored-tsvector variants (a_stored_and, b_stored_or,
# d_bm25_stored) and per-scope latency at 10k and 100k, added after the
# first two runs; the 1M and multi-tenant runs include them already.
SK=b_simple_or_rank,b_simple_or_rank_cd,d_bm25_querytime,d_bm25_maintained,trgm,vector
$PY run.py --scale 10000 --skip $SK --out results/s10k_stored.json > $LOG/s10k_stored.log 2>&1
$PY run.py --scale 100000 --skip $SK --out results/s100k_stored.json > $LOG/s100k_stored.log 2>&1
