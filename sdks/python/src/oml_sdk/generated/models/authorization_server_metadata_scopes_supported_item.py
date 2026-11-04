from enum import StrEnum


class AuthorizationServerMetadataScopesSupportedItem(StrEnum):
    MCP = "mcp"

    def __str__(self) -> str:
        return str(self.value)
