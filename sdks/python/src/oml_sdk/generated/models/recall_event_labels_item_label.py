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

    @classmethod
    def _missing_(cls, value: object) -> "RecallEventLabelsItemLabel | None":
        # A value a later server adds is kept as the string it is, never
        # refused: an SDK from v0.8.5 on reads a newer server's answers
        # (docs/engineering/sdks.md, forward compatibility).
        if not isinstance(value, str):
            return None
        member = str.__new__(cls, value)
        member._name_ = value
        member._value_ = value
        return member
