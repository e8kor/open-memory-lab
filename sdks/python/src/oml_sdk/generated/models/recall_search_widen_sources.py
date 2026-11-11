from enum import StrEnum


class RecallSearchWidenSources(StrEnum):
    WIDE = "wide"

    def __str__(self) -> str:
        return str(self.value)
