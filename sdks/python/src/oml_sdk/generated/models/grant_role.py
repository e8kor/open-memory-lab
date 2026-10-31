from enum import StrEnum


class GrantRole(StrEnum):
    DREAMER = "dreamer"
    READER = "reader"
    WRITER = "writer"

    def __str__(self) -> str:
        return str(self.value)
