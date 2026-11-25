from enum import StrEnum


class UsageDriftDriftItemMeter(StrEnum):
    CONNECTIONS = "connections"
    KEYS = "keys"
    MEMORIES = "memories"
    TEXT_BYTES = "text_bytes"
    WORKSPACES = "workspaces"
    WRITES = "writes"

    def __str__(self) -> str:
        return str(self.value)
