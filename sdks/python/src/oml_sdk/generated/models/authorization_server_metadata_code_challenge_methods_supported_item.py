from enum import StrEnum


class AuthorizationServerMetadataCodeChallengeMethodsSupportedItem(StrEnum):
    S256 = "S256"

    def __str__(self) -> str:
        return str(self.value)
