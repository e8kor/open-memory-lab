from enum import StrEnum


class MemorySummaryProvenance(StrEnum):
    ADMIN_FIX = "admin_fix"
    AGENT = "agent"
    CONNECTOR = "connector"
    HUMAN = "human"
    SIMULATION = "simulation"

    def __str__(self) -> str:
        return str(self.value)
