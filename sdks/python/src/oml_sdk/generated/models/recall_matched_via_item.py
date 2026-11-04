from enum import StrEnum


class RecallMatchedViaItem(StrEnum):
    BM25 = "bm25"
    KEYWORD_ALL = "keyword_all"
    KEYWORD_ANY = "keyword_any"
    TRIGRAM = "trigram"

    def __str__(self) -> str:
        return str(self.value)
