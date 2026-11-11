from enum import StrEnum


class RecallSourcesMode(StrEnum):
    LIST = "list"
    WIDE = "wide"

    def __str__(self) -> str:
        return str(self.value)
