from enum import StrEnum


class GuardrailScope(StrEnum):
    ANCESTORS = "ancestors"
    WORKSPACE = "workspace"

    def __str__(self) -> str:
        return str(self.value)
