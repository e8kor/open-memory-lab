from enum import StrEnum


class UploadSessionState(StrEnum):
    ABANDONED = "abandoned"
    BOUND = "bound"
    COMPLETED = "completed"
    EXPIRED = "expired"
    FAILED = "failed"
    OPEN = "open"

    def __str__(self) -> str:
        return str(self.value)
