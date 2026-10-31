from enum import StrEnum


class TermMappingPredicate(StrEnum):
    BROADMATCH = "broadMatch"
    CLOSEMATCH = "closeMatch"
    EXACTMATCH = "exactMatch"
    NARROWMATCH = "narrowMatch"
    RELATEDMATCH = "relatedMatch"

    def __str__(self) -> str:
        return str(self.value)
