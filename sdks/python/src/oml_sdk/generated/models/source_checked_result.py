from enum import StrEnum


class SourceCheckedResult(StrEnum):
    CHANGED = "changed"
    EQUIVALENT = "equivalent"
    FRESH = "fresh"
    MOVED = "moved"
    REMOVED = "removed"

    def __str__(self) -> str:
        return str(self.value)

    @classmethod
    def _missing_(cls, value: object) -> "SourceCheckedResult | None":
        # A value a later server adds is kept as the string it is, never
        # refused: an SDK from v0.8.5 on reads a newer server's answers
        # (docs/engineering/sdks.md, forward compatibility).
        if not isinstance(value, str):
            return None
        member = str.__new__(cls, value)
        member._name_ = value
        member._value_ = value
        return member
