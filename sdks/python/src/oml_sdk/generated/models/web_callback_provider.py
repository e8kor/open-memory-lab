from enum import StrEnum


class WebCallbackProvider(StrEnum):
    GITHUB = "github"
    GOOGLE = "google"

    def __str__(self) -> str:
        return str(self.value)
