from enum import StrEnum


class WorkareaViewChildrenItemStatus(StrEnum):
    COMMITTED = "committed"
    DEVELOPMENT = "development"
    VALIDATED = "validated"

    def __str__(self) -> str:
        return str(self.value)
