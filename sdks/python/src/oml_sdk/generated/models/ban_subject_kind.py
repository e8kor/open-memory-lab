from enum import StrEnum


class BanSubjectKind(StrEnum):
    PRINCIPAL = "principal"
    SOURCE = "source"

    def __str__(self) -> str:
        return str(self.value)
