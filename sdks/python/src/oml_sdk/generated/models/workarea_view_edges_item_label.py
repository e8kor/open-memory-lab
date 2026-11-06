from enum import StrEnum


class WorkareaViewEdgesItemLabel(StrEnum):
    CONTINUES = "continues"
    DERIVED_FROM = "derived_from"
    MIRRORS = "mirrors"
    RELATES_TO = "relates_to"

    def __str__(self) -> str:
        return str(self.value)
