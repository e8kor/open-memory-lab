from enum import StrEnum


class BanRequestSubjectKind(StrEnum):
    PRINCIPAL = "principal"
    SOURCE = "source"

    def __str__(self) -> str:
        return str(self.value)
