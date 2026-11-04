from enum import StrEnum


class ProtectedResourceMetadataBearerMethodsSupportedItem(StrEnum):
    HEADER = "header"

    def __str__(self) -> str:
        return str(self.value)
