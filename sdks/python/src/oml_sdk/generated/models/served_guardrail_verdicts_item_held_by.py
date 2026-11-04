from enum import StrEnum


class ServedGuardrailVerdictsItemHeldBy(StrEnum):
    AGENT = "agent"
    HUMAN = "human"
    SERVICE = "service"

    def __str__(self) -> str:
        return str(self.value)
