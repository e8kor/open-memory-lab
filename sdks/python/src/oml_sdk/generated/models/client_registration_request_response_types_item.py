from enum import StrEnum


class ClientRegistrationRequestResponseTypesItem(StrEnum):
    CODE = "code"

    def __str__(self) -> str:
        return str(self.value)
