from enum import StrEnum


class SearchMemoriesKind(StrEnum):
    DERIVED_FACT = "derived_fact"
    MEMORY = "memory"
    STEERING = "steering"
    TERM = "term"
    TERM_MAPPING = "term_mapping"
    VOCAB_RELEASE = "vocab_release"

    def __str__(self) -> str:
        return str(self.value)
