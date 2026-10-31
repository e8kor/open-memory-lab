from enum import StrEnum


class RecallRequestKindsItem(StrEnum):
    DERIVED_FACT = "derived_fact"
    MEMORY = "memory"

    def __str__(self) -> str:
        return str(self.value)
