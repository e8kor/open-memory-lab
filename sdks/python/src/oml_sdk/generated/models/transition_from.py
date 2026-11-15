from enum import StrEnum


class TransitionFrom(StrEnum):
    DEVELOPMENT = "development"
    PROTECTED = "protected"

    def __str__(self) -> str:
        return str(self.value)
