from enum import StrEnum


class RecallBudgetCounter(StrEnum):
    OML_TC1 = "oml-tc/1"

    def __str__(self) -> str:
        return str(self.value)
