from enum import StrEnum


class GrantRole(StrEnum):
    READER = "reader"
    REVIEWER = "reviewer"
    WRITER = "writer"

    def __str__(self) -> str:
        return str(self.value)
