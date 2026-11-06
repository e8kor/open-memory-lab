from enum import StrEnum


class AnchorHasher(StrEnum):
    OML_CRATE1 = "oml-crate/1"
    OML_MD1 = "oml-md/1"
    OML_RUSTDOC1 = "oml-rustdoc/1"
    OML_TURN1 = "oml-turn/1"

    def __str__(self) -> str:
        return str(self.value)
