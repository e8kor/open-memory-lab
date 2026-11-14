from enum import StrEnum


class WorkareaViewEdgesItemState(StrEnum):
    PENDING = "pending"
    RATIFIED = "ratified"

    def __str__(self) -> str:
        return str(self.value)
