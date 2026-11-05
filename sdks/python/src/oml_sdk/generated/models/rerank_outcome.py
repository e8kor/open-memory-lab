from enum import StrEnum


class RerankOutcome(StrEnum):
    APPLIED = "applied"
    SKIPPED = "skipped"

    def __str__(self) -> str:
        return str(self.value)
