from enum import StrEnum


class UserPageActBodyAction(StrEnum):
    SIGN_OUT = "sign_out"

    def __str__(self) -> str:
        return str(self.value)
