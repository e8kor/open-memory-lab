from enum import StrEnum


class TransitionFrom(StrEnum):
    COMMITTED = "committed"
    DEVELOPMENT = "development"

    def __str__(self) -> str:
        return str(self.value)
