from enum import StrEnum


class Role(StrEnum):
    ADMIN = "admin"
    DREAMER = "dreamer"
    OPERATOR = "operator"
    READER = "reader"
    WRITER = "writer"

    def __str__(self) -> str:
        return str(self.value)
