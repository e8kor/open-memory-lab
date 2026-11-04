from enum import StrEnum


class OAuthErrorError(StrEnum):
    INVALID_CLIENT = "invalid_client"
    INVALID_CLIENT_METADATA = "invalid_client_metadata"
    INVALID_GRANT = "invalid_grant"
    INVALID_REDIRECT_URI = "invalid_redirect_uri"
    INVALID_REQUEST = "invalid_request"
    INVALID_SCOPE = "invalid_scope"
    INVALID_TARGET = "invalid_target"
    UNSUPPORTED_GRANT_TYPE = "unsupported_grant_type"

    def __str__(self) -> str:
        return str(self.value)
