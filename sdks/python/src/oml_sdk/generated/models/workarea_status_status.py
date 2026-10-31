from enum import StrEnum


class WorkareaStatusStatus(StrEnum):
    COMMITTED = "committed"
    DEVELOPMENT = "development"
    VALIDATED = "validated"

    def __str__(self) -> str:
        return str(self.value)
