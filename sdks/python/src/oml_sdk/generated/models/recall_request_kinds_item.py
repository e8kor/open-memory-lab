from enum import StrEnum


class RecallRequestKindsItem(StrEnum):
    DERIVED_FACT = "derived_fact"
    DOCUMENT = "document"
    MEMORY = "memory"

    def __str__(self) -> str:
        return str(self.value)
