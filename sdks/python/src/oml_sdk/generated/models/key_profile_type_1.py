from enum import StrEnum


class KeyProfileType1(StrEnum):
    CURATOR = "curator"

    def __str__(self) -> str:
        return str(self.value)
