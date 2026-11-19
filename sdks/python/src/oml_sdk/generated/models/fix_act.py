from enum import StrEnum


class FixAct(StrEnum):
    DEMOTE = "demote"
    EDIT = "edit"
    ERASE = "erase"
    RETIRE = "retire"

    def __str__(self) -> str:
        return str(self.value)
