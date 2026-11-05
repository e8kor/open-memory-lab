from enum import StrEnum


class Stale(StrEnum):
    CHANGED = "changed"
    REMOVED = "removed"

    def __str__(self) -> str:
        return str(self.value)
