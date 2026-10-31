from enum import StrEnum


class TransitionTo(StrEnum):
    COMMITTED = "committed"
    VALIDATED = "validated"

    def __str__(self) -> str:
        return str(self.value)
