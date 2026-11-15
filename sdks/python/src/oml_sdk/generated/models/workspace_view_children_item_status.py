from enum import StrEnum


class WorkspaceViewChildrenItemStatus(StrEnum):
    DEVELOPMENT = "development"
    FROZEN = "frozen"
    PROTECTED = "protected"

    def __str__(self) -> str:
        return str(self.value)
