from enum import StrEnum


class ConnectionState(StrEnum):
    OPEN = "open"
    REVOKED = "revoked"

    def __str__(self) -> str:
        return str(self.value)
