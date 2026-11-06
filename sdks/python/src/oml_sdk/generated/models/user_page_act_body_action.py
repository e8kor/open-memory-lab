from enum import StrEnum


class UserPageActBodyAction(StrEnum):
    CONNECT_CODE = "connect_code"
    CONNECT_KEY = "connect_key"
    CREATE_WORKAREA = "create_workarea"
    CUT_OFF = "cut_off"
    ISSUE_KEY = "issue_key"
    REVIEW_NEXT = "review_next"
    SIGN_OUT = "sign_out"
    VERDICT = "verdict"
    WORKAREA_MODE = "workarea_mode"
    WORKAREA_RATIFY = "workarea_ratify"
    WORKAREA_TRANSITION = "workarea_transition"

    def __str__(self) -> str:
        return str(self.value)
