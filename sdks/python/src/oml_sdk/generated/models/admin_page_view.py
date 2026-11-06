from enum import StrEnum


class AdminPageView(StrEnum):
    CURATE = "curate"
    HELD = "held"
    MEMORIES = "memories"
    PEOPLE = "people"
    QUEUE = "queue"
    REVIEW = "review"
    WORKAREA = "workarea"

    def __str__(self) -> str:
        return str(self.value)
