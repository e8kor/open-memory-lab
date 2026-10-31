from enum import StrEnum


class SubmitRequestVerb(StrEnum):
    LEARN = "learn"
    REMEMBER = "remember"

    def __str__(self) -> str:
        return str(self.value)
