from enum import StrEnum


class WhoamiProfileType1(StrEnum):
    CURATOR = "curator"

    def __str__(self) -> str:
        return str(self.value)
