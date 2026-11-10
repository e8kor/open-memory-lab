from enum import StrEnum


class ListedKeyCredential(StrEnum):
    BEARER = "bearer"
    CONNECTION = "connection"
    SESSION = "session"

    def __str__(self) -> str:
        return str(self.value)
