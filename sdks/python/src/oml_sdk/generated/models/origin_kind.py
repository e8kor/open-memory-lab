from enum import StrEnum


class OriginKind(StrEnum):
    CHAT = "chat"
    CODE = "code"
    DOC = "doc"
    LAKE = "lake"
    MANUAL = "manual"
    REPO = "repo"
    WEB = "web"

    def __str__(self) -> str:
        return str(self.value)
