from enum import StrEnum


class ListMemoriesKind(StrEnum):
    DERIVED_FACT = "derived_fact"
    DOCUMENT = "document"
    MEMORY = "memory"
    SKILL = "skill"
    STEERING = "steering"
    TERM = "term"
    TERM_MAPPING = "term_mapping"
    TEST = "test"
    VOCAB_RELEASE = "vocab_release"

    def __str__(self) -> str:
        return str(self.value)
