from enum import StrEnum


class VerdictRequestVerdict(StrEnum):
    APPROVE = "approve"
    REJECT = "reject"

    def __str__(self) -> str:
        return str(self.value)
