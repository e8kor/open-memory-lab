from enum import StrEnum


class BuildProfile(StrEnum):
    DEBUG = "debug"
    RELEASE = "release"
    UNKNOWN = "unknown"

    def __str__(self) -> str:
        return str(self.value)
