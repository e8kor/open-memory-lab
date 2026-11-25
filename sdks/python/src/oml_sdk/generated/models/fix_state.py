from enum import StrEnum


class FixState(StrEnum):
    CONFIRMED = "confirmed"
    REVERTED = "reverted"
    SUPERSEDED = "superseded"
    UNCONFIRMED = "unconfirmed"

    def __str__(self) -> str:
        return str(self.value)

    @classmethod
    def _missing_(cls, value: object) -> "FixState | None":
        # A value a later server adds is kept as the string it is, never
        # refused: an SDK from v0.8.5 on reads a newer server's answers
        # (docs/engineering/sdks.md, forward compatibility).
        if not isinstance(value, str):
            return None
        member = str.__new__(cls, value)
        member._name_ = value
        member._value_ = value
        return member
