from enum import StrEnum


class RerankReason(StrEnum):
    BUSY = "busy"
    FAILED = "failed"
    TIMEOUT = "timeout"

    def __str__(self) -> str:
        return str(self.value)
