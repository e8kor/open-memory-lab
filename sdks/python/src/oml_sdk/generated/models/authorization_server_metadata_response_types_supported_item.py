from enum import StrEnum


class AuthorizationServerMetadataResponseTypesSupportedItem(StrEnum):
    CODE = "code"

    def __str__(self) -> str:
        return str(self.value)
