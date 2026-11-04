from enum import StrEnum


class TokenResponseScope(StrEnum):
    MCP = "mcp"

    def __str__(self) -> str:
        return str(self.value)
