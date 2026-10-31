from enum import StrEnum


class SupersedeRequestTermStatus(StrEnum):
    ACTIVE = "active"
    DEPRECATED = "deprecated"

    def __str__(self) -> str:
        return str(self.value)
