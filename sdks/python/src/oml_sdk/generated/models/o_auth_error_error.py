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

    @classmethod
    def _missing_(cls, value: object) -> "OAuthErrorError | None":
        # A value a later server adds is kept as the string it is, never
        # refused: an SDK from v0.8.5 on reads a newer server's answers
        # (docs/engineering/sdks.md, forward compatibility).
        if not isinstance(value, str):
            return None
        member = str.__new__(cls, value)
        member._name_ = value
        member._value_ = value
        return member
