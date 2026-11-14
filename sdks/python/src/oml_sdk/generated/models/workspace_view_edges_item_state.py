from enum import StrEnum


class WorkspaceViewEdgesItemState(StrEnum):
    PENDING = "pending"
    RATIFIED = "ratified"

    def __str__(self) -> str:
        return str(self.value)
