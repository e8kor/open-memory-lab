from enum import StrEnum


class WorkareaOffersRefusedItemTo(StrEnum):
    COMMITTED = "committed"
    IN_PROGRESS = "in_progress"
    STANDARD = "standard"
    VALIDATED = "validated"

    def __str__(self) -> str:
        return str(self.value)
