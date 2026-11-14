from enum import StrEnum


class WorkareaOffersRefusedItemOp(StrEnum):
    MODE = "mode"
    PROPOSE = "propose"

    def __str__(self) -> str:
        return str(self.value)
