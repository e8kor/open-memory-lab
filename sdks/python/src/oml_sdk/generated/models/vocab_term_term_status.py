from enum import StrEnum


class VocabTermTermStatus(StrEnum):
    ACTIVE = "active"
    DEPRECATED = "deprecated"

    def __str__(self) -> str:
        return str(self.value)
