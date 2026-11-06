from enum import StrEnum


class WorkareaMode(StrEnum):
    IN_PROGRESS = "in_progress"
    STANDARD = "standard"

    def __str__(self) -> str:
        return str(self.value)
