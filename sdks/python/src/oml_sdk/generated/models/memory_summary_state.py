from enum import StrEnum


class MemorySummaryState(StrEnum):
    DEMOTED = "demoted"
    ERASED = "erased"
    HELD = "held"
    PROMOTED = "promoted"
    REJECTED = "rejected"
    WITHDRAWN = "withdrawn"

    def __str__(self) -> str:
        return str(self.value)
