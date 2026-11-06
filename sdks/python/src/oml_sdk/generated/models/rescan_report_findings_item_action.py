from enum import StrEnum


class RescanReportFindingsItemAction(StrEnum):
    HOLD = "hold"
    VETO = "veto"

    def __str__(self) -> str:
        return str(self.value)
