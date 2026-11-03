from enum import StrEnum


class UploadSessionState(StrEnum):
    ABORTED = "aborted"
    COMPLETE = "complete"
    EXPIRED = "expired"
    OPEN = "open"

    def __str__(self) -> str:
        return str(self.value)
