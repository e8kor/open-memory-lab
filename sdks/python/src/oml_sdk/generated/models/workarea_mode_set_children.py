from enum import StrEnum


class WorkareaModeSetChildren(StrEnum):
    AGENT = "agent"
    NONE = "none"

    def __str__(self) -> str:
        return str(self.value)
