from enum import StrEnum


class AdminMemoryState(StrEnum):
    DEMOTED = "demoted"
    HELD = "held"
    PROMOTED = "promoted"
    REJECTED = "rejected"
    SUPERSEDED = "superseded"
    WITHDRAWN = "withdrawn"

    def __str__(self) -> str:
        return str(self.value)
