from enum import StrEnum


class GuardrailRuleTool(StrEnum):
    EDIT = "edit"
    FETCH = "fetch"
    MCP = "mcp"
    READ = "read"
    SHELL = "shell"

    def __str__(self) -> str:
        return str(self.value)
