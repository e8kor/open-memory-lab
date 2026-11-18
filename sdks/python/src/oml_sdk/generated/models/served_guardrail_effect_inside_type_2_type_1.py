from enum import StrEnum


class ServedGuardrailEffectInsideType2Type1(StrEnum):
    ASK = "ask"

    def __str__(self) -> str:
        return str(self.value)
