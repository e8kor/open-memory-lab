from enum import StrEnum


class AdminPageActBodyAction(StrEnum):
    ALLOW_EMAIL = "allow_email"
    BULK_CONFIRM = "bulk_confirm"
    BULK_VERDICT = "bulk_verdict"
    CURATE = "curate"
    CUT_OFF = "cut_off"
    GRANT_CURATION = "grant_curation"
    GRANT_REVIEW = "grant_review"
    REMOVE_EMAIL = "remove_email"
    REVIEW_NEXT = "review_next"
    SET_ADMIN = "set_admin"
    SUPERSEDE = "supersede"
    VERDICT = "verdict"
    WORKAREA_MODE = "workarea_mode"
    WORKAREA_RATIFY = "workarea_ratify"
    WORKAREA_TRANSITION = "workarea_transition"

    def __str__(self) -> str:
        return str(self.value)
