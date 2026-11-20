from enum import StrEnum


class AdminListFixesState(StrEnum):
    ALL = "all"
    UNCONFIRMED = "unconfirmed"

    def __str__(self) -> str:
        return str(self.value)
