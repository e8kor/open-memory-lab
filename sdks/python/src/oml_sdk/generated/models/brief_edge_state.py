from enum import StrEnum


class BriefEdgeState(StrEnum):
    PENDING = "pending"
    RATIFIED = "ratified"

    def __str__(self) -> str:
        return str(self.value)
