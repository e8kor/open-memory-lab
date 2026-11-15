from enum import StrEnum


class ListMemoriesState(StrEnum):
    DEMOTED = "demoted"
    ERASED = "erased"
    HELD = "held"
    PROMOTED = "promoted"
    REJECTED = "rejected"
    WITHDRAWN = "withdrawn"

    def __str__(self) -> str:
        return str(self.value)
