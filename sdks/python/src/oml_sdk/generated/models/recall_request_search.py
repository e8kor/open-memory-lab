from enum import StrEnum


class RecallRequestSearch(StrEnum):
    BM25 = "bm25"
    BM25_RERANK = "bm25-rerank"
    BM25_TRIGRAM = "bm25-trigram"
    KEYWORD_ALL = "keyword-all"
    KEYWORD_ANY = "keyword-any"

    def __str__(self) -> str:
        return str(self.value)
