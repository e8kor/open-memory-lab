from enum import StrEnum


class WorkspaceOffersRefusedItemOp(StrEnum):
    MODE = "mode"
    PROPOSE = "propose"

    def __str__(self) -> str:
        return str(self.value)
