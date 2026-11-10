from enum import StrEnum


class ReadyWebClient(StrEnum):
    MTLS = "mtls"
    NONE = "none"
    SECRET = "secret"
    SECRETMTLS = "secret+mtls"

    def __str__(self) -> str:
        return str(self.value)
