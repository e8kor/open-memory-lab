from enum import StrEnum


class StartStatusResponse200FirstMemoryType3Type1(StrEnum):
    HELD = "held"
    PROMOTED = "promoted"

    def __str__(self) -> str:
        return str(self.value)
