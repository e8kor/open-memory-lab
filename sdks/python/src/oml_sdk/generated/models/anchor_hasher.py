from enum import StrEnum


class AnchorHasher(StrEnum):
    OML_DELTA_COL1 = "oml-delta-col/1"
    OML_DELTA_SCHEMA1 = "oml-delta-schema/1"
    OML_DELTA_TABLE1 = "oml-delta-table/1"
    OML_MD1 = "oml-md/1"
    OML_TURN1 = "oml-turn/1"

    def __str__(self) -> str:
        return str(self.value)
