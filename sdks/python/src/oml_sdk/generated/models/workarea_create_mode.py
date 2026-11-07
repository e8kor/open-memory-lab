from enum import StrEnum


class WorkareaCreateMode(StrEnum):
    IN_PROGRESS = "in_progress"

    def __str__(self) -> str:
        return str(self.value)
