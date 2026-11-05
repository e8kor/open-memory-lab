#!/usr/bin/env bash
# Fetches a reranker's pinned files into <dir> when they are absent and
# checks each against the SHA-256 the lead pins (crates/adapters/oml-rerank;
# ADR-0053 d3), so CI and an offline install's staging host hold exactly the
# measured model. A download is checked before it is moved into place, so a
# bad one never stands where the lead or a later run would take it for the
# model; a file already present that does not match stops the script.
#
#   scripts/fetch_rerank_model.sh <dir> [ms-marco-MiniLM-L6-v2 | ms-marco-MiniLM-L4-v2]
set -euo pipefail
DIR="${1:?usage: fetch_rerank_model.sh <dir> [model]}"
MODEL="${2:-ms-marco-MiniLM-L6-v2}"
case "$MODEL" in
    ms-marco-MiniLM-L6-v2)
        REV="233902d25c440f23af6f7d6e94d2946bac0bee0a"
        SUMS="380e02c93f431831be65d99a4e7e5f67c133985bf2e77d9d4eba46847190bacc config.json
d241a60d5e8f04cc1b2b3e9ef7a4921b27bf526d9f6050ab90f9267a1f9e5c66 tokenizer.json
821d1aa69520101d6e0737f78a042ae25b19e5cb9160701909d10434f4aeb0ae model.safetensors" ;;
    ms-marco-MiniLM-L4-v2)
        REV="777b2f369bc1c2f850df8bd367ed1654bda4497b"
        SUMS="0349c3fb99c7c59d75529c12932e7663bf1d27ca4d8ee0a5202410466deba657 config.json
d241a60d5e8f04cc1b2b3e9ef7a4921b27bf526d9f6050ab90f9267a1f9e5c66 tokenizer.json
3db1c98132b54901976d6da56fa711756c7780a6263d5db8f336232584f5fe58 model.safetensors" ;;
    *) echo "fetch_rerank_model.sh: $MODEL is not a model this build pins" >&2; exit 2 ;;
esac
REPO="cross-encoder/$MODEL"
mkdir -p "$DIR"
while read -r sum file; do
    if [ ! -f "$DIR/$file" ]; then
        curl -fsSL -o "$DIR/$file.partial" "https://huggingface.co/$REPO/resolve/$REV/$file"
        if ! echo "$sum  $DIR/$file.partial" | sha256sum -c --quiet -; then
            rm -f "$DIR/$file.partial"
            echo "fetch_rerank_model.sh: $file from $REPO@$REV is not the pinned file; nothing was moved into place" >&2
            exit 1
        fi
        mv "$DIR/$file.partial" "$DIR/$file"
    fi
    echo "$sum  $DIR/$file" | sha256sum -c --quiet -
done <<<"$SUMS"
echo "$MODEL files in $DIR match their pinned digests"
