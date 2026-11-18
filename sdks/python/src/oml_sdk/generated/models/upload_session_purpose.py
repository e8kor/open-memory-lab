from enum import StrEnum


class UploadSessionPurpose(StrEnum):
    IMPORT = "import"

    def __str__(self) -> str:
        return str(self.value)
