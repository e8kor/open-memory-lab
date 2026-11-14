from enum import StrEnum


class KeyIssueRequestUsedBy(StrEnum):
    AGENT = "agent"
    PERSON = "person"
    SERVICE = "service"

    def __str__(self) -> str:
        return str(self.value)
