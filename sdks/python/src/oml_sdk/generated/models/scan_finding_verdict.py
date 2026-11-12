from enum import StrEnum


class ScanFindingVerdict(StrEnum):
    ANNOTATE = "annotate"
    HOLD = "hold"
    REFUSE = "refuse"

    def __str__(self) -> str:
        return str(self.value)
