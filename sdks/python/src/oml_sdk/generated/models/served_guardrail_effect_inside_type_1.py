from enum import StrEnum


class ServedGuardrailEffectInsideType1(StrEnum):
    ASK = "ask"

    def __str__(self) -> str:
        return str(self.value)
