from enum import StrEnum


class SupersedeRequestOnSimilar(StrEnum):
    HOLD = "hold"
    REFUSE = "refuse"

    def __str__(self) -> str:
        return str(self.value)
