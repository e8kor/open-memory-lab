from enum import StrEnum


class WorkspaceOffersRefusedItemTo(StrEnum):
    FROZEN = "frozen"
    IN_PROGRESS = "in_progress"
    PROTECTED = "protected"
    STANDARD = "standard"

    def __str__(self) -> str:
        return str(self.value)
