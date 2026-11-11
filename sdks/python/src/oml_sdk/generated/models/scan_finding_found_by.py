from enum import StrEnum


class ScanFindingFoundBy(StrEnum):
    ADMISSION = "admission"
    RESCAN = "rescan"

    def __str__(self) -> str:
        return str(self.value)
