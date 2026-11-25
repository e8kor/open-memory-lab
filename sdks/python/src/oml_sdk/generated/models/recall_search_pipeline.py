from enum import StrEnum


class RecallSearchPipeline(StrEnum):
    BM25 = "bm25"
    BM25_POSTINGS = "bm25-postings"
    BM25_POSTINGS_TIME = "bm25-postings-time"
    BM25_RERANK = "bm25-rerank"
    BM25_SPARSE = "bm25-sparse"
    BM25_SPARSE_RRF = "bm25-sparse-rrf"
    BM25_TRIGRAM = "bm25-trigram"
    KEYWORD_ALL = "keyword-all"
    KEYWORD_ANY = "keyword-any"
    SPARSE = "sparse"

    def __str__(self) -> str:
        return str(self.value)

    @classmethod
    def _missing_(cls, value: object) -> "RecallSearchPipeline | None":
        # A value a later server adds is kept as the string it is, never
        # refused: an SDK from v0.8.5 on reads a newer server's answers
        # (docs/engineering/sdks.md, forward compatibility).
        if not isinstance(value, str):
            return None
        member = str.__new__(cls, value)
        member._name_ = value
        member._value_ = value
        return member
