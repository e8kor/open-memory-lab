#!/usr/bin/env bash
# Fetches the reranker's pinned files into <dir> when they are absent and
# checks each against the SHA-256 the lead pins (crates/adapters/oml-rerank,
# MINILM_L6; ADR-0053 d3), so CI and an offline install's staging host hold
# exactly the measured model. A file that does not match stops the script.
#
#   scripts/fetch_rerank_model.sh <dir>
set -euo pipefail
DIR="${1:?usage: fetch_rerank_model.sh <dir>}"
REPO="cross-encoder/ms-marco-MiniLM-L6-v2"
REV="233902d25c440f23af6f7d6e94d2946bac0bee0a"
mkdir -p "$DIR"
while read -r sum file; do
    if [ ! -f "$DIR/$file" ]; then
        curl -fsSL -o "$DIR/$file.partial" "https://huggingface.co/$REPO/resolve/$REV/$file"
        mv "$DIR/$file.partial" "$DIR/$file"
    fi
    echo "$sum  $DIR/$file" | sha256sum -c --quiet -
done <<'SUMS'
380e02c93f431831be65d99a4e7e5f67c133985bf2e77d9d4eba46847190bacc config.json
d241a60d5e8f04cc1b2b3e9ef7a4921b27bf526d9f6050ab90f9267a1f9e5c66 tokenizer.json
821d1aa69520101d6e0737f78a042ae25b19e5cb9160701909d10434f4aeb0ae model.safetensors
SUMS
echo "reranker files in $DIR match their pinned digests"
