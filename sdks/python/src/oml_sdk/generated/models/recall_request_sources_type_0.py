from enum import StrEnum


class RecallRequestSourcesType0(StrEnum):
    SCOPE = "scope"
    WIDE = "wide"

    def __str__(self) -> str:
        return str(self.value)
