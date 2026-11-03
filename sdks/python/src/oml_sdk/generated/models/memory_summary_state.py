from enum import StrEnum


class MemorySummaryState(StrEnum):
    DEMOTED = "demoted"
    ERASED = "erased"
    HELD = "held"
    PROMOTED = "promoted"
    REJECTED = "rejected"

    def __str__(self) -> str:
        return str(self.value)
