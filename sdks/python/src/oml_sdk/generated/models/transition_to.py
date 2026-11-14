from enum import StrEnum


class TransitionTo(StrEnum):
    FROZEN = "frozen"
    PROTECTED = "protected"

    def __str__(self) -> str:
        return str(self.value)
