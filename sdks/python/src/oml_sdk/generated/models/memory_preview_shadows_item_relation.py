from enum import StrEnum


class MemoryPreviewShadowsItemRelation(StrEnum):
    SHADOWED_BY = "shadowed_by"
    SHADOWS = "shadows"

    def __str__(self) -> str:
        return str(self.value)
