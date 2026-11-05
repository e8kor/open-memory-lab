from enum import StrEnum


class RecallEventLabelsItemSource(StrEnum):
    EXPLICIT = "explicit"
    INFERRED = "inferred"

    def __str__(self) -> str:
        return str(self.value)
