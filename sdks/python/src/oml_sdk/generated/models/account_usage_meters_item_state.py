from enum import StrEnum


class AccountUsageMetersItemState(StrEnum):
    OK = "ok"
    REACHED = "reached"
    WARNING = "warning"

    def __str__(self) -> str:
        return str(self.value)
