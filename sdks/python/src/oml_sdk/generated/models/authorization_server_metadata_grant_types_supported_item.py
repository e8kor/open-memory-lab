from enum import StrEnum


class AuthorizationServerMetadataGrantTypesSupportedItem(StrEnum):
    AUTHORIZATION_CODE = "authorization_code"
    REFRESH_TOKEN = "refresh_token"

    def __str__(self) -> str:
        return str(self.value)
