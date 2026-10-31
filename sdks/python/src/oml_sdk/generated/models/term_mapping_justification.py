from enum import StrEnum


class TermMappingJustification(StrEnum):
    HUMAN_CURATED = "human-curated"
    LEXICAL = "lexical"
    LOGICAL = "logical"

    def __str__(self) -> str:
        return str(self.value)
