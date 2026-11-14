from enum import StrEnum


class GrantRequestRole(StrEnum):
    READER = "reader"
    REVIEWER = "reviewer"
    WRITER = "writer"

    def __str__(self) -> str:
        return str(self.value)
