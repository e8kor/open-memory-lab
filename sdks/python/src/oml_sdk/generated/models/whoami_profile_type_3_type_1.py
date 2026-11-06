from enum import StrEnum


class WhoamiProfileType3Type1(StrEnum):
    CURATOR = "curator"

    def __str__(self) -> str:
        return str(self.value)
