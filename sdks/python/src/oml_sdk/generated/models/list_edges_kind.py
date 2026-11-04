from enum import StrEnum


class ListEdgesKind(StrEnum):
    BRIEF = "brief"
    CLAIM = "claim"

    def __str__(self) -> str:
        return str(self.value)
