from enum import StrEnum


class TransitionProposalTo(StrEnum):
    FROZEN = "frozen"
    PROTECTED = "protected"

    def __str__(self) -> str:
        return str(self.value)
