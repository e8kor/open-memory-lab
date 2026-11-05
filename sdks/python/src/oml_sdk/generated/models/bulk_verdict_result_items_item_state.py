from enum import StrEnum


class BulkVerdictResultItemsItemState(StrEnum):
    PROMOTED = "promoted"
    REJECTED = "rejected"

    def __str__(self) -> str:
        return str(self.value)
