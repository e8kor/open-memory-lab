from enum import StrEnum


class VocabTermDefinitionState(StrEnum):
    ERASED = "erased"
    PRESENT = "present"

    def __str__(self) -> str:
        return str(self.value)
