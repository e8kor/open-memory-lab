from enum import StrEnum


class SubmitRequestOnSimilar(StrEnum):
    HOLD = "hold"
    REFUSE = "refuse"

    def __str__(self) -> str:
        return str(self.value)
