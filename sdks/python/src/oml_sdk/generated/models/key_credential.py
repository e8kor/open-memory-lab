from enum import StrEnum


class KeyCredential(StrEnum):
    BEARER = "bearer"
    CONNECTION = "connection"

    def __str__(self) -> str:
        return str(self.value)
