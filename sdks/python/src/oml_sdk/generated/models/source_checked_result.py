from enum import StrEnum


class SourceCheckedResult(StrEnum):
    CHANGED = "changed"
    EQUIVALENT = "equivalent"
    FRESH = "fresh"
    MOVED = "moved"
    REMOVED = "removed"

    def __str__(self) -> str:
        return str(self.value)
