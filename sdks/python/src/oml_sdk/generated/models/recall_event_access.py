from enum import StrEnum


class RecallEventAccess(StrEnum):
    NONE = "none"
    POSTINGS = "postings"
    SCAN = "scan"

    def __str__(self) -> str:
        return str(self.value)
