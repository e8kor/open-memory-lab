from enum import StrEnum


class WhoamiGrantRole(StrEnum):
    READER = "reader"
    REVIEWER = "reviewer"
    SUBMITTER = "submitter"

    def __str__(self) -> str:
        return str(self.value)
