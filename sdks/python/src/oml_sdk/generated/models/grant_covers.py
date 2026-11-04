from enum import StrEnum


class GrantCovers(StrEnum):
    NODE = "node"
    SUBTREE = "subtree"

    def __str__(self) -> str:
        return str(self.value)
