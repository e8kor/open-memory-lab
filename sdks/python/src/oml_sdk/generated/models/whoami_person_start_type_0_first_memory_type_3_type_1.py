from enum import StrEnum


class WhoamiPersonStartType0FirstMemoryType3Type1(StrEnum):
    HELD = "held"
    PROMOTED = "promoted"

    def __str__(self) -> str:
        return str(self.value)
