from enum import StrEnum


class TokenRequestScope(StrEnum):
    MCP = "mcp"

    def __str__(self) -> str:
        return str(self.value)
