from enum import StrEnum


class LineageEdgesItemKind(StrEnum):
    CLAIM_EDGE = "claim_edge"
    DERIVES_FROM = "derives_from"

    def __str__(self) -> str:
        return str(self.value)
