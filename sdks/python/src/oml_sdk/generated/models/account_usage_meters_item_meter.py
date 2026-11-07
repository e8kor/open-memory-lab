from enum import StrEnum


class AccountUsageMetersItemMeter(StrEnum):
    CONNECTIONS = "connections"
    KEYS = "keys"
    MEMORIES = "memories"
    RECALLS = "recalls"
    TEXT_BYTES = "text_bytes"
    WRITES = "writes"

    def __str__(self) -> str:
        return str(self.value)
