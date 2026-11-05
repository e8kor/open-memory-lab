from enum import StrEnum


class RecallSearchAccess(StrEnum):
    POSTINGS = "postings"
    SCAN = "scan"

    def __str__(self) -> str:
        return str(self.value)
