from enum import StrEnum


class WhoamiGrantCovers(StrEnum):
    NODE = "node"
    SUBTREE = "subtree"

    def __str__(self) -> str:
        return str(self.value)
