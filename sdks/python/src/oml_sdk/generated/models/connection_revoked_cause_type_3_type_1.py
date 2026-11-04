from enum import StrEnum


class ConnectionRevokedCauseType3Type1(StrEnum):
    ADMIN = "admin"
    CODE_REUSE = "code_reuse"
    REFRESH_REUSE = "refresh_reuse"
    REPLACED = "replaced"

    def __str__(self) -> str:
        return str(self.value)
