from enum import StrEnum


class WorkareaStatus(StrEnum):
    COMMITTED = "committed"
    DEVELOPMENT = "development"
    VALIDATED = "validated"

    def __str__(self) -> str:
        return str(self.value)
