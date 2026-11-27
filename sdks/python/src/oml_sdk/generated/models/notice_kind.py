from enum import StrEnum


class NoticeKind(StrEnum):
    ACT_AGAINST_ADMIN = "act_against_admin"
    OFFENSIVE_NARROWING = "offensive_narrowing"
    RECOVERY = "recovery"
    ROOT_REACH = "root_reach"
    SETTLEMENT = "settlement"

    def __str__(self) -> str:
        return str(self.value)

    @classmethod
    def _missing_(cls, value: object) -> "NoticeKind | None":
        # A value a later server adds is kept as the string it is, never
        # refused: an SDK from v0.8.5 on reads a newer server's answers
        # (docs/engineering/sdks.md, forward compatibility).
        if not isinstance(value, str):
            return None
        member = str.__new__(cls, value)
        member._name_ = value
        member._value_ = value
        return member
