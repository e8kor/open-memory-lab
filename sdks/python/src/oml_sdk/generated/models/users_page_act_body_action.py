from enum import StrEnum


class UsersPageActBodyAction(StrEnum):
    CLOSE_DELETION = "close_deletion"
    DISABLE_USER = "disable_user"
    SET_QUOTA = "set_quota"

    def __str__(self) -> str:
        return str(self.value)
