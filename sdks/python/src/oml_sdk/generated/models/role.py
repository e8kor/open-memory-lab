from enum import StrEnum


class Role(StrEnum):
    ADMIN = "admin"
    CONTRIBUTOR = "contributor"
    OPERATOR = "operator"
    READER = "reader"
    REVIEWER = "reviewer"
    SUBMITTER = "submitter"

    def __str__(self) -> str:
        return str(self.value)
