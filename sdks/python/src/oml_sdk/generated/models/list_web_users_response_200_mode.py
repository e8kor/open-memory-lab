from enum import StrEnum


class ListWebUsersResponse200Mode(StrEnum):
    ALLOWLIST = "allowlist"
    OPEN = "open"

    def __str__(self) -> str:
        return str(self.value)
