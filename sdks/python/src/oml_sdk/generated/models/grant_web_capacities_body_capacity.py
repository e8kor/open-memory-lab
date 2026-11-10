from enum import StrEnum


class GrantWebCapacitiesBodyCapacity(StrEnum):
    CURATE = "curate"
    REVIEW = "review"

    def __str__(self) -> str:
        return str(self.value)
