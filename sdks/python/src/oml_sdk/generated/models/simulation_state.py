from enum import StrEnum


class SimulationState(StrEnum):
    ENDED = "ended"
    OPEN = "open"

    def __str__(self) -> str:
        return str(self.value)
