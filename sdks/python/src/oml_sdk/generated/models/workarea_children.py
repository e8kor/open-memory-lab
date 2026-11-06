from enum import StrEnum


class WorkareaChildren(StrEnum):
    AGENT = "agent"
    NONE = "none"

    def __str__(self) -> str:
        return str(self.value)
