from enum import StrEnum


class ApproverAct(StrEnum):
    ADMIN_FIX = "admin_fix"
    VERDICT = "verdict"

    def __str__(self) -> str:
        return str(self.value)
