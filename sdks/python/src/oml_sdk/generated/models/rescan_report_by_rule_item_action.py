from enum import StrEnum


class RescanReportByRuleItemAction(StrEnum):
    HOLD = "hold"
    VETO = "veto"

    def __str__(self) -> str:
        return str(self.value)
