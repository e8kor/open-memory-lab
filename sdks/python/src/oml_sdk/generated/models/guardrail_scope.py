from enum import StrEnum


class GuardrailScope(StrEnum):
    ANCESTORS = "ancestors"
    WORKAREA = "workarea"

    def __str__(self) -> str:
        return str(self.value)
