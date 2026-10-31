from enum import StrEnum


class SearchMemoriesState(StrEnum):
    DEMOTED = "demoted"
    ERASED = "erased"
    HELD = "held"
    PROMOTED = "promoted"
    REJECTED = "rejected"

    def __str__(self) -> str:
        return str(self.value)
