from enum import StrEnum


class KeyIssueRequestProfile(StrEnum):
    CURATOR = "curator"

    def __str__(self) -> str:
        return str(self.value)
