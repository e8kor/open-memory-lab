from enum import StrEnum


class SetFederationPolicyBodyDefaultSources(StrEnum):
    SCOPE = "scope"
    WIDE = "wide"

    def __str__(self) -> str:
        return str(self.value)
