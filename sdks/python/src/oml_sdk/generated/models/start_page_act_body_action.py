from enum import StrEnum


class StartPageActBodyAction(StrEnum):
    ISSUE_CLAUDE_AI = "issue_claude_ai"
    ISSUE_CLAUDE_CODE = "issue_claude_code"
    ISSUE_OTHER = "issue_other"
    REQUEST_DELETION = "request_deletion"

    def __str__(self) -> str:
        return str(self.value)
