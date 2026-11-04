from enum import StrEnum


class UserPageView(StrEnum):
    KEYS = "keys"
    MEMORIES = "memories"
    REVIEW = "review"
    SETUP = "setup"
    WORKAREA = "workarea"

    def __str__(self) -> str:
        return str(self.value)
