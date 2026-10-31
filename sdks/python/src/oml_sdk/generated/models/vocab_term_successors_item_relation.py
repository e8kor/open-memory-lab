from enum import StrEnum


class VocabTermSuccessorsItemRelation(StrEnum):
    CONSIDER = "consider"
    REPLACED_BY = "replaced_by"

    def __str__(self) -> str:
        return str(self.value)
