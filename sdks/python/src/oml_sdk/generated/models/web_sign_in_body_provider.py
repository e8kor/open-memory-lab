from enum import StrEnum


class WebSignInBodyProvider(StrEnum):
    GITHUB = "github"
    GOOGLE = "google"

    def __str__(self) -> str:
        return str(self.value)
