from enum import StrEnum


class ListedKeyHeldBy(StrEnum):
    AGENT = "agent"
    HUMAN = "human"
    SERVICE = "service"

    def __str__(self) -> str:
        return str(self.value)
