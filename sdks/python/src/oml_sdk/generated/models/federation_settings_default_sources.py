from enum import StrEnum


class FederationSettingsDefaultSources(StrEnum):
    SCOPE = "scope"
    WIDE = "wide"

    def __str__(self) -> str:
        return str(self.value)
