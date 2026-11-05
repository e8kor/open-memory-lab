from enum import StrEnum


class RecallEventLabelsItemLabel(StrEnum):
    DERIVED = "derived"
    FEEDBACK_NOT_USEFUL = "feedback_not_useful"
    FEEDBACK_USEFUL = "feedback_useful"
    GET = "get"
    NO_USE = "no_use"
    REQUERY = "requery"
    SUPERSEDED = "superseded"

    def __str__(self) -> str:
        return str(self.value)
