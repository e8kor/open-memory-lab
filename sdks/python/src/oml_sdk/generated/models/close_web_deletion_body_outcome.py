from enum import StrEnum


class CloseWebDeletionBodyOutcome(StrEnum):
    DELETED = "deleted"

    def __str__(self) -> str:
        return str(self.value)
