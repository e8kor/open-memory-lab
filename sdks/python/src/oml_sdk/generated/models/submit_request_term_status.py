from enum import StrEnum


class SubmitRequestTermStatus(StrEnum):
    ACTIVE = "active"
    DEPRECATED = "deprecated"

    def __str__(self) -> str:
        return str(self.value)
