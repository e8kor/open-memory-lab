from enum import StrEnum


class TransitionProposalTo(StrEnum):
    COMMITTED = "committed"
    VALIDATED = "validated"

    def __str__(self) -> str:
        return str(self.value)
