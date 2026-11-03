from enum import StrEnum


class LineageEdgesItemState(StrEnum):
    PENDING = "pending"
    RATIFIED = "ratified"

    def __str__(self) -> str:
        return str(self.value)
