from enum import StrEnum


class WorkspaceOffersTransitionsItemOp(StrEnum):
    PROPOSE = "propose"
    RATIFY = "ratify"

    def __str__(self) -> str:
        return str(self.value)
