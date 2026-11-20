from enum import StrEnum


class AdminFixBodyAct(StrEnum):
    DEMOTE = "demote"
    EDIT = "edit"
    ERASE = "erase"
    RETIRE = "retire"

    def __str__(self) -> str:
        return str(self.value)
