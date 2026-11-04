from enum import StrEnum


class AuthorizeResponseType(StrEnum):
    CODE = "code"

    def __str__(self) -> str:
        return str(self.value)
