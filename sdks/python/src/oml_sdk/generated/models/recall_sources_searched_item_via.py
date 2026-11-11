from enum import StrEnum


class RecallSourcesSearchedItemVia(StrEnum):
    FEDERATED = "federated"
    SCOPE = "scope"

    def __str__(self) -> str:
        return str(self.value)
