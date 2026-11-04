from enum import StrEnum


class MemorySummarySeverity(StrEnum):
    MUST = "must"
    NEVER = "never"
    SHOULD = "should"

    def __str__(self) -> str:
        return str(self.value)
