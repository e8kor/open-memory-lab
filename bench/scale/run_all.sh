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
SKV=bm25,dual,trgm,a_simple_and,b_simple_or_rank,b_simple_or_rank_cd,d_bm25_querytime
# 1M vectors as halfvec(384): float vector(384) at 1M with its HNSW index would
# not fit beside the memories in the 18 GB build filesystem; 100k is measured
# both ways to show what halfvec changes.
$PY run.py --scale 1000000 --skip $SKV --halfvec --timeout-ms 5000 --norls-one-pass --out results/s1m_vector.json > $LOG/s1m_vector.log 2>&1
$PY run.py --scale 100000 --skip $SKV --halfvec --out results/s100k_halfvec.json > $LOG/s100k_halfvec.log 2>&1
# Supplement: the stored-tsvector variants (a_stored_and, b_stored_or,
# d_bm25_stored) and per-scope latency at 10k and 100k, added after the
# first two runs; the 1M runs include them, the multi-tenant run does not.
SK=b_simple_or_rank,b_simple_or_rank_cd,d_bm25_querytime,d_bm25_maintained,trgm,vector
$PY run.py --scale 10000 --skip $SK --out results/s10k_stored.json > $LOG/s10k_stored.log 2>&1
$PY run.py --scale 100000 --skip $SK --out results/s100k_stored.json > $LOG/s100k_stored.log 2>&1
# The 1M text run stopped at the BM25 RLS check (a 5 s statement timeout on a
# correctness count, since lifted for that check); its measured approaches are
# kept in s1m_text.json and the rest resumed here.
$PY run.py --scale 1000000 --skip vector,a_simple_and,b_simple_or_rank,b_simple_or_rank_cd,d_bm25_querytime,d_bm25_maintained --timeout-ms 5000 --norls-one-pass --out results/s1m_text2.json > $LOG/s1m_text2.log 2>&1
# Added cases (extra.py): exact vector scan vs HNSW under selective scopes,
# and a learned-sparse inverted table at 30k, 100k and 200k; 1M sparse is
# extrapolated (60M postings would not fit beside the other builds sharing
# the filesystem).
$PY extra.py exact --scale 10000 --out results/x_exact_10k.json > $LOG/x.log 2>&1
$PY extra.py exact --scale 100000 --out results/x_exact_100k.json >> $LOG/x.log 2>&1
$PY extra.py sparse --scale 30000 --out results/x_sparse_30k.json >> $LOG/x.log 2>&1
$PY extra.py sparse --scale 100000 --out results/x_sparse_100k.json >> $LOG/x.log 2>&1
$PY extra.py sparse --scale 200000 --out results/x_sparse_200k.json >> $LOG/x.log 2>&1
