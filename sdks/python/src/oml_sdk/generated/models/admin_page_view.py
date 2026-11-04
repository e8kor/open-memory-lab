from enum import StrEnum


class AdminPageView(StrEnum):
    CURATE = "curate"
    MEMORIES = "memories"
    PEOPLE = "people"
    QUEUE = "queue"
    REVIEW = "review"

    def __str__(self) -> str:
        return str(self.value)
