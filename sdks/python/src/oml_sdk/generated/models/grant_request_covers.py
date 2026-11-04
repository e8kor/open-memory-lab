from enum import StrEnum


class GrantRequestCovers(StrEnum):
    NODE = "node"
    SUBTREE = "subtree"

    def __str__(self) -> str:
        return str(self.value)
