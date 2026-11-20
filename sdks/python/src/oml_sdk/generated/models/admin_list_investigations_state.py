from enum import StrEnum


class AdminListInvestigationsState(StrEnum):
    CLOSED = "closed"
    OPEN = "open"

    def __str__(self) -> str:
        return str(self.value)
