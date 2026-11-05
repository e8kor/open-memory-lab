from enum import StrEnum


class AnchorHasher(StrEnum):
    OML_MD1 = "oml-md/1"

    def __str__(self) -> str:
        return str(self.value)
