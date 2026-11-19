from enum import StrEnum


class FixState(StrEnum):
    CONFIRMED = "confirmed"
    REVERTED = "reverted"
    SUPERSEDED = "superseded"
    UNCONFIRMED = "unconfirmed"

    def __str__(self) -> str:
        return str(self.value)
