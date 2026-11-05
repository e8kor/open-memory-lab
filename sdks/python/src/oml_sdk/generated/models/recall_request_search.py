from enum import StrEnum


class RecallRequestSearch(StrEnum):
    BM25 = "bm25"
    BM25_POSTINGS = "bm25-postings"
    BM25_RERANK = "bm25-rerank"
    BM25_SPARSE = "bm25-sparse"
    BM25_SPARSE_RRF = "bm25-sparse-rrf"
    BM25_TRIGRAM = "bm25-trigram"
    KEYWORD_ALL = "keyword-all"
    KEYWORD_ANY = "keyword-any"
    SPARSE = "sparse"

    def __str__(self) -> str:
        return str(self.value)
