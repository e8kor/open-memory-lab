from enum import StrEnum


class ImportPlanKeysItemOutcome(StrEnum):
    CHANGED = "changed"
    CONFLICT = "conflict"
    EQUIVALENT = "equivalent"
    NEW = "new"
    REMOVED = "removed"
    UNCHANGED = "unchanged"

    def __str__(self) -> str:
        return str(self.value)
