from enum import StrEnum


class ExportRequestMode(StrEnum):
    HISTORY = "history"
    RELEASE = "release"

    def __str__(self) -> str:
        return str(self.value)
