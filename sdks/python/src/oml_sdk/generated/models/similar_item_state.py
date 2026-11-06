from enum import StrEnum


class SimilarItemState(StrEnum):
    HELD = "held"
    PROMOTED = "promoted"

    def __str__(self) -> str:
        return str(self.value)
