from enum import StrEnum


class WhoamiHeldBy(StrEnum):
    AGENT = "agent"
    HUMAN = "human"
    SERVICE = "service"

    def __str__(self) -> str:
        return str(self.value)
