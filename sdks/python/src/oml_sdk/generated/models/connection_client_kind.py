from enum import StrEnum


class ConnectionClientKind(StrEnum):
    CIMD = "cimd"
    DCR = "dcr"

    def __str__(self) -> str:
        return str(self.value)
