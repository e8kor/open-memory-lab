from enum import StrEnum


class UsersPageActBodyQuota(StrEnum):
    CONNECTIONS = "connections"
    KEYS = "keys"
    MEMORIES = "memories"
    REQUESTS_PER_MINUTE = "requests_per_minute"

    def __str__(self) -> str:
        return str(self.value)
