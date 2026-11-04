from enum import StrEnum


class GuardrailRuleRule(StrEnum):
    OML_GR1 = "oml-gr/1"

    def __str__(self) -> str:
        return str(self.value)
