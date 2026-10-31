from enum import StrEnum


class RecallRequestExpandTerms(StrEnum):
    DESCENDANTS = "descendants"
    NONE = "none"

    def __str__(self) -> str:
        return str(self.value)
