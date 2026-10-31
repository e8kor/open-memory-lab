from enum import StrEnum


class MemorySummaryProvenance(StrEnum):
    AGENT = "agent"
    CONNECTOR = "connector"
    HUMAN = "human"
    SIMULATION = "simulation"

    def __str__(self) -> str:
        return str(self.value)
