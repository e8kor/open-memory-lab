from enum import StrEnum


class ReviewNextBodyNeeds(StrEnum):
    PERSON = "person"
    REVIEWER = "reviewer"

    def __str__(self) -> str:
        return str(self.value)
