from enum import StrEnum


class RecallSearchHits(StrEnum):
    NOT_REQUESTED = "not_requested"

    def __str__(self) -> str:
        return str(self.value)
