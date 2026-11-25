from enum import StrEnum


class ConnectionRevokedCauseType3Type1(StrEnum):
    ADMIN = "admin"
    CODE_REUSE = "code_reuse"
    REFRESH_REUSE = "refresh_reuse"
    REPLACED = "replaced"

    def __str__(self) -> str:
        return str(self.value)

    @classmethod
    def _missing_(cls, value: object) -> "ConnectionRevokedCauseType3Type1 | None":
        # A value a later server adds is kept as the string it is, never
        # refused: an SDK from v0.8.5 on reads a newer server's answers
        # (docs/engineering/sdks.md, forward compatibility).
        if not isinstance(value, str):
            return None
        member = str.__new__(cls, value)
        member._name_ = value
        member._value_ = value
        return member
