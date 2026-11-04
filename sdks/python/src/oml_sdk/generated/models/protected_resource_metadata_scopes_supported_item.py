from enum import StrEnum


class ProtectedResourceMetadataScopesSupportedItem(StrEnum):
    MCP = "mcp"

    def __str__(self) -> str:
        return str(self.value)
