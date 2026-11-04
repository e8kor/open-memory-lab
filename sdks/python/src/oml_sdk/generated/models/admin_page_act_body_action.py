from enum import StrEnum


class AdminPageActBodyAction(StrEnum):
    ALLOW_EMAIL = "allow_email"
    CURATE = "curate"
    CUT_OFF = "cut_off"
    GRANT_CURATION = "grant_curation"
    GRANT_REVIEW = "grant_review"
    REMOVE_EMAIL = "remove_email"
    REVIEW_NEXT = "review_next"
    SET_ADMIN = "set_admin"
    SUPERSEDE = "supersede"
    VERDICT = "verdict"

    def __str__(self) -> str:
        return str(self.value)
