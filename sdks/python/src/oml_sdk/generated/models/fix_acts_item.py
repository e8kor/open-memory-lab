from enum import StrEnum


class FixActsItem(StrEnum):
    CONFIRM = "confirm"
    REVERT = "revert"

    def __str__(self) -> str:
        return str(self.value)
