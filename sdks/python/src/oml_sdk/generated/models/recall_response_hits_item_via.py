from enum import StrEnum


class RecallResponseHitsItemVia(StrEnum):
    FEDERATED = "federated"
    SCOPE = "scope"

    def __str__(self) -> str:
        return str(self.value)
