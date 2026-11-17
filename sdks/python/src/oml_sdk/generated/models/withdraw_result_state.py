from enum import StrEnum


class WithdrawResultState(StrEnum):
    WITHDRAWN = "withdrawn"

    def __str__(self) -> str:
        return str(self.value)
