from enum import StrEnum


class RecallMatchedViaItem(StrEnum):
    BM25 = "bm25"
    BM25_POSTINGS = "bm25_postings"
    KEYWORD_ALL = "keyword_all"
    KEYWORD_ANY = "keyword_any"
    SPARSE = "sparse"
    TIME = "time"
    TRIGRAM = "trigram"

    def __str__(self) -> str:
        return str(self.value)
