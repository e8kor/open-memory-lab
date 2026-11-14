from enum import StrEnum


class Role(StrEnum):
    ADMIN = "admin"
    OPERATOR = "operator"
    READER = "reader"
    REVIEWER = "reviewer"
    WRITER = "writer"

    def __str__(self) -> str:
        return str(self.value)
