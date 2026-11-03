from enum import StrEnum


class KeyHeldByType3Type1(StrEnum):
    AGENT = "agent"
    HUMAN = "human"
    SERVICE = "service"

    def __str__(self) -> str:
        return str(self.value)
