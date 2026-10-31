from enum import StrEnum


class VerdictResultState(StrEnum):
    HELD = "held"
    PROMOTED = "promoted"
    REJECTED = "rejected"

    def __str__(self) -> str:
        return str(self.value)
