from enum import StrEnum


class StartPageClient(StrEnum):
    CLAUDE_AI = "claude-ai"
    CLAUDE_CODE = "claude-code"
    OTHER = "other"

    def __str__(self) -> str:
        return str(self.value)
