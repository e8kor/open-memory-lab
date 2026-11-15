from enum import StrEnum


class TestFieldsShape(StrEnum):
    DOCUMENT = "document"
    GUARDRAIL = "guardrail"
    RECALL = "recall"
    SKILL = "skill"

    def __str__(self) -> str:
        return str(self.value)
