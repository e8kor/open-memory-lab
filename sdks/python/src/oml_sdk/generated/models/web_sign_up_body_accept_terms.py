from enum import StrEnum


class WebSignUpBodyAcceptTerms(StrEnum):
    YES = "yes"

    def __str__(self) -> str:
        return str(self.value)
