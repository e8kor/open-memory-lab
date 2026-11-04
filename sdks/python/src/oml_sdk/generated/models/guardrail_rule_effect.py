from enum import StrEnum


class GuardrailRuleEffect(StrEnum):
    ASK = "ask"
    DENY = "deny"

    def __str__(self) -> str:
        return str(self.value)
