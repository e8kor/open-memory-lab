from enum import StrEnum


class StartStatusResponse200FirstMemoryType1(StrEnum):
    HELD = "held"
    PROMOTED = "promoted"

    def __str__(self) -> str:
        return str(self.value)
