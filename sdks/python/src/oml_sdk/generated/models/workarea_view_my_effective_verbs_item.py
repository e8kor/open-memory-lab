from enum import StrEnum


class WorkareaViewMyEffectiveVerbsItem(StrEnum):
    ATTACH = "attach"
    EVENTS_TAIL = "events_tail"
    GRAPH_READ = "graph_read"
    LEARN = "learn"
    PROPOSE = "propose"
    READ = "read"
    RECALL = "recall"
    REMEMBER = "remember"
    REVIEW = "review"
    SUPERSEDE = "supersede"

    def __str__(self) -> str:
        return str(self.value)
