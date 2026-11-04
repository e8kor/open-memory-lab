from enum import StrEnum


class KeyCredential(StrEnum):
    BEARER = "bearer"
    CONNECTION = "connection"
    SESSION = "session"

    def __str__(self) -> str:
        return str(self.value)
