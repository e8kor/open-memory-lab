from enum import StrEnum


class AuthorizeSubmitBodyType1Action(StrEnum):
    CANCEL = "cancel"
    CONNECT = "connect"

    def __str__(self) -> str:
        return str(self.value)
