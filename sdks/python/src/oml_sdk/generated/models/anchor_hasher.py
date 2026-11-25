from enum import StrEnum


class AnchorHasher(StrEnum):
    OML_CRATE1 = "oml-crate/1"
    OML_DELTA_COL1 = "oml-delta-col/1"
    OML_DELTA_SCHEMA1 = "oml-delta-schema/1"
    OML_DELTA_TABLE1 = "oml-delta-table/1"
    OML_MD1 = "oml-md/1"
    OML_RUSTDOC1 = "oml-rustdoc/1"
    OML_TURN1 = "oml-turn/1"

    def __str__(self) -> str:
        return str(self.value)

    @classmethod
    def _missing_(cls, value: object) -> "AnchorHasher | None":
        # A value a later server adds is kept as the string it is, never
        # refused: an SDK from v0.8.5 on reads a newer server's answers
        # (docs/engineering/sdks.md, forward compatibility).
        if not isinstance(value, str):
            return None
        member = str.__new__(cls, value)
        member._name_ = value
        member._value_ = value
        return member
