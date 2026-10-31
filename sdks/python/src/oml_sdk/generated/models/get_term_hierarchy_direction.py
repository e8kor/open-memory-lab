from enum import StrEnum


class GetTermHierarchyDirection(StrEnum):
    ANCESTORS = "ancestors"
    CHILDREN = "children"
    DESCENDANTS = "descendants"
    PARENTS = "parents"

    def __str__(self) -> str:
        return str(self.value)
