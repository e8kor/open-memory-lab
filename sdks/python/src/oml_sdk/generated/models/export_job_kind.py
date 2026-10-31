from enum import StrEnum


class ExportJobKind(StrEnum):
    CHAIN = "chain"
    EVIDENCE = "evidence"

    def __str__(self) -> str:
        return str(self.value)
