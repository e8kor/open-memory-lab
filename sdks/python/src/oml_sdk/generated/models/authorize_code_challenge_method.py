from enum import StrEnum


class AuthorizeCodeChallengeMethod(StrEnum):
    S256 = "S256"

    def __str__(self) -> str:
        return str(self.value)
