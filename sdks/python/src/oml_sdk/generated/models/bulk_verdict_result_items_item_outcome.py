from enum import StrEnum


class BulkVerdictResultItemsItemOutcome(StrEnum):
    APPROVED = "approved"
    REJECTED = "rejected"
    SKIPPED = "skipped"

    def __str__(self) -> str:
        return str(self.value)
