from enum import StrEnum


class ListedKeyUsedBy(StrEnum):
    AGENT = "agent"
    PERSON = "person"
    SERVICE = "service"

    def __str__(self) -> str:
        return str(self.value)
