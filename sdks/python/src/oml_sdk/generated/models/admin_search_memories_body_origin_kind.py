from enum import StrEnum


class AdminSearchMemoriesBodyOriginKind(StrEnum):
    CHAT = "chat"
    CODE = "code"
    DOC = "doc"
    LAKE = "lake"
    MANUAL = "manual"
    REPO = "repo"
    WEB = "web"

    def __str__(self) -> str:
        return str(self.value)

    @classmethod
    def _missing_(cls, value: object) -> "AdminSearchMemoriesBodyOriginKind | None":
        # A value a later server adds is kept as the string it is, never
        # refused: an SDK from v0.8.5 on reads a newer server's answers
        # (docs/engineering/sdks.md, forward compatibility).
        if not isinstance(value, str):
            return None
        member = str.__new__(cls, value)
        member._name_ = value
        member._value_ = value
        return member
