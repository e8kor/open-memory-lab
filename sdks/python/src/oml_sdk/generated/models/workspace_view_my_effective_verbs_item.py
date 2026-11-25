from enum import StrEnum


class WorkspaceViewMyEffectiveVerbsItem(StrEnum):
    ATTACH = "attach"
    CREATE_WORKSPACE = "create_workspace"
    EVENTS_TAIL = "events_tail"
    GRAPH_READ = "graph_read"
    LEARN = "learn"
    PROPOSE = "propose"
    PROPOSE_BRIEF = "propose_brief"
    READ = "read"
    RECALL = "recall"
    REMEMBER = "remember"
    REVIEW = "review"
    SUPERSEDE = "supersede"

    def __str__(self) -> str:
        return str(self.value)
