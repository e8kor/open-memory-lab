from enum import StrEnum


class ClientRegistrationRequestTokenEndpointAuthMethod(StrEnum):
    NONE = "none"

    def __str__(self) -> str:
        return str(self.value)
