from enum import StrEnum


class SimilarItemRelation(StrEnum):
    NEAR_COPY = "near_copy"
    SAME_TEXT = "same_text"

    def __str__(self) -> str:
        return str(self.value)
