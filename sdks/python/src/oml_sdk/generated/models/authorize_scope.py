from enum import StrEnum


class AuthorizeScope(StrEnum):
    MCP = "mcp"

    def __str__(self) -> str:
        return str(self.value)
