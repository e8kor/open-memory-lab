from enum import StrEnum


class SimilarItemKind(StrEnum):
    DERIVED_FACT = "derived_fact"
    MEMORY = "memory"
    STEERING = "steering"

    def __str__(self) -> str:
        return str(self.value)
