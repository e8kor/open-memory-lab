from enum import StrEnum


class KeyUsedByType2Type1(StrEnum):
    AGENT = "agent"
    PERSON = "person"
    SERVICE = "service"

    def __str__(self) -> str:
        return str(self.value)
