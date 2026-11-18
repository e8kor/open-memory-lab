from enum import StrEnum


class UploadOpenRequestPurpose(StrEnum):
    IMPORT = "import"

    def __str__(self) -> str:
        return str(self.value)
