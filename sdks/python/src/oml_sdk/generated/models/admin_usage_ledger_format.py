from enum import StrEnum


class AdminUsageLedgerFormat(StrEnum):
    CSV = "csv"
    JSON = "json"

    def __str__(self) -> str:
        return str(self.value)
