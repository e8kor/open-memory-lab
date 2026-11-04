from enum import StrEnum


class AuthorizationServerMetadataTokenEndpointAuthMethodsSupportedItem(StrEnum):
    NONE = "none"

    def __str__(self) -> str:
        return str(self.value)
