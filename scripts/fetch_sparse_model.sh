#!/usr/bin/env bash
# Fetches the learned sparse encoder's pinned files into <dir> when they are
# absent and checks each against the SHA-256 the lead pins
# (crates/adapters/oml-rerank/src/sparse.rs; ADR-0052 d14), as
# fetch_rerank_model.sh does for the reranker: a download is checked before
# it is moved into place, and a file already present that does not match
# stops the script. The model is Apache-2.0 (its model card).
#
#   scripts/fetch_sparse_model.sh <dir>
set -euo pipefail
DIR="${1:?usage: fetch_sparse_model.sh <dir>}"
REPO="opensearch-project/opensearch-neural-sparse-encoding-doc-v3-distill"
REV="babf71f3c48695e2e53a978208e8aba48335e3c0"
SUMS="ee97780493e7d0a3b7b788ea98f3391e6be6b0b379921b465ca55bfdd0d9cbe3 config.json
91f1def9b9391fdabe028cd3f3fcc4efd34e5d1f08c3bf2de513ebb5911a1854 tokenizer.json
83a3cc9757876b8590aac53f4f6685012f89d7fb4bbeb540815a54d325f7f70a model.safetensors
da23a1c0b9252776cc8c6d70fd14723e218f484d489cd9027ac6e4065d5b9edd idf.json"
mkdir -p "$DIR"
while read -r sum file; do
    if [ ! -f "$DIR/$file" ]; then
        curl -fsSL -o "$DIR/$file.partial" "https://huggingface.co/$REPO/resolve/$REV/$file"
        if ! echo "$sum  $DIR/$file.partial" | sha256sum -c --quiet -; then
            rm -f "$DIR/$file.partial"
            echo "fetch_sparse_model.sh: $file from $REPO@$REV is not the pinned file; nothing was moved into place" >&2
            exit 1
        fi
        mv "$DIR/$file.partial" "$DIR/$file"
    fi
    echo "$sum  $DIR/$file" | sha256sum -c --quiet -
done <<<"$SUMS"
echo "doc-v3-distill files in $DIR match their pinned digests"
