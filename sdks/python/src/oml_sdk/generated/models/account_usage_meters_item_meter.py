from enum import StrEnum


class AccountUsageMetersItemMeter(StrEnum):
    CONNECTIONS = "connections"
    KEYS = "keys"
    MEMORIES = "memories"
    RECALLS = "recalls"
    TEXT_BYTES = "text_bytes"
    WORKSPACES = "workspaces"
    WRITES = "writes"

    def __str__(self) -> str:
        return str(self.value)

    @classmethod
    def _missing_(cls, value: object) -> "AccountUsageMetersItemMeter | None":
        # A value a later server adds is kept as the string it is, never
        # refused: an SDK from v0.8.5 on reads a newer server's answers
        # (docs/engineering/sdks.md, forward compatibility).
        if not isinstance(value, str):
            return None
        member = str.__new__(cls, value)
        member._name_ = value
        member._value_ = value
        return member
