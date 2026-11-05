from enum import StrEnum


class RecallSearchPipeline(StrEnum):
    BM25 = "bm25"
    BM25_POSTINGS = "bm25-postings"
    BM25_POSTINGS_TIME = "bm25-postings-time"
    BM25_POSTINGS_TIME_RECENT = "bm25-postings-time-recent"
    BM25_RERANK = "bm25-rerank"
    BM25_SPARSE = "bm25-sparse"
    BM25_TRIGRAM = "bm25-trigram"
    KEYWORD_ALL = "keyword-all"
    KEYWORD_ANY = "keyword-any"
    SPARSE = "sparse"

    def __str__(self) -> str:
        return str(self.value)
