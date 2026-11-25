from enum import StrEnum


class ClaimKind(StrEnum):
    DERIVED_FACT = "derived_fact"
    DOCUMENT = "document"
    GUARDRAIL = "guardrail"
    MEMORY = "memory"
    SKILL = "skill"
    STEERING = "steering"
    TERM = "term"
    TERM_MAPPING = "term_mapping"
    TEST = "test"
    VOCAB_RELEASE = "vocab_release"

    def __str__(self) -> str:
        return str(self.value)

    @classmethod
    def _missing_(cls, value: object) -> "ClaimKind | None":
        # A value a later server adds is kept as the string it is, never
        # refused: an SDK from v0.8.5 on reads a newer server's answers
        # (docs/engineering/sdks.md, forward compatibility).
        if not isinstance(value, str):
            return None
        member = str.__new__(cls, value)
        member._name_ = value
        member._value_ = value
        return member
