from enum import StrEnum


class ListedKeyGrantsItemCovers(StrEnum):
    NODE = "node"
    SUBTREE = "subtree"

    def __str__(self) -> str:
        return str(self.value)
