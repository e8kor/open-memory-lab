"""A fake `OmlClient` for oml_sdk.langchain's unit tests: one workspace, the
rules the store relies on, and a record of every call (ADR-0094).

It holds the server's rules as docs/architecture/audit-and-lineage.md and
ADR-0030 state them: a held version is not a tip and cannot be superseded; a
claim key is taken while a held or promoted version holds it; a withdrawn
version releases it; only the author withdraws, and only a held version; the
scoped listing inside an in-progress workspace serves every author's held rows.
"""

from __future__ import annotations

import datetime
import itertools
from types import SimpleNamespace

from oml_sdk.errors import refused_locally
from oml_sdk.generated.models import Memory

_ids = itertools.count(1)
_clock = itertools.count(1)


def refusal(status: int, code: str, message: str = "refused") -> Exception:
    return refused_locally(status, code, message, "the fake's rule", None)


class FakeOml:
    def __init__(self, *, key: str = "agent", in_progress: bool = True, rows=None) -> None:
        self.key = key
        self.in_progress = in_progress
        self.rows: list[dict] = rows if rows is not None else []
        self.calls: list[tuple[str, dict]] = []

    def as_key(self, key: str) -> FakeOml:
        """Another key over the same rows."""
        other = FakeOml(key=key, in_progress=self.in_progress, rows=self.rows)
        other.calls = self.calls
        return other

    # -- seeding --------------------------------------------------------------

    def seed(
        self,
        claim_key: str,
        text: str,
        *,
        state: str,
        author: str,
        version: int = 1,
        retirement: bool = False,
        anchor: dict | None = None,
    ):
        row = {
            "artifact_id": f"art_{next(_ids)}",
            "claim_key": claim_key,
            "version": version,
            "state": state,
            "text": text,
            "author": author,
            "retirement": retirement,
            "anchor": anchor,
            "created_at": datetime.datetime(2026, 11, 24, tzinfo=datetime.UTC)
            + datetime.timedelta(seconds=next(_clock)),
        }
        self.rows.append(row)
        return row

    def memory(self, row: dict) -> Memory:
        return Memory.from_dict(
            {
                "artifact_id": row["artifact_id"],
                "claim_key": row["claim_key"],
                "version": row["version"],
                "tip": row["state"] == "promoted",
                "workspace_id": "ws_progress",
                "workspace_path": "universe.sdk-progress",
                "kind": "memory",
                "state": row["state"],
                "unreviewed": row["state"] == "held",
                "tokens": 1,
                "retirement": row["retirement"],
                "replaces": None,
                "held_expires_at": None,
                "created_at": row["created_at"].isoformat(),
                "promoted_at": row["created_at"].isoformat()
                if row["state"] == "promoted"
                else None,
                "anchor": row["anchor"],
                "stale": None,
                "source_checked": None,
                "text": row["text"],
                "format": "application/json",
            }
        )

    # -- the client's surface -------------------------------------------------

    def workspace(self, workspace=None):
        self.calls.append(("workspace", {"workspace": workspace}))
        mode = "in_progress" if self.in_progress else "standard"
        root = SimpleNamespace(id="ws_progress", name="sdk-progress") if self.in_progress else None
        return SimpleNamespace(
            id="ws_progress", name="sdk-progress", mode=mode, in_progress_root=root
        )

    def _visible(self, row: dict) -> bool:
        if row["state"] == "promoted":
            return True
        return row["state"] == "held" and (self.in_progress or row["author"] == self.key)

    def iter_memories(self, *, own=None, claim_key=None, prefix=None, **filters):
        self.calls.append(("iter_memories", {"own": own, "claim_key": claim_key, "prefix": prefix}))
        for row in self.rows:
            if claim_key is not None and row["claim_key"] != claim_key:
                continue
            if prefix is not None and not row["claim_key"].startswith(prefix):
                continue
            if own and row["author"] != self.key:
                continue
            if not own and not self._visible(row):
                continue
            yield self.memory(row)

    def read(self, artifact_id=None, *, query=None, include_unreviewed=None, **filters):
        self.calls.append(
            ("read", {"query": query, "include_unreviewed": include_unreviewed} | filters)
        )
        words = set(query.lower().split())
        hits = [
            SimpleNamespace(memory=self.memory(row), score=1.0)
            for row in self.rows
            if (row["state"] == "promoted" or (include_unreviewed and self._visible(row)))
            and words & set(row["text"].lower().replace('"', " ").split())
        ]
        return SimpleNamespace(hits=hits, steering=[])

    def write(self, text, *, claim_key=None, replaces=None, expected_version=None, **fields):
        self.calls.append(
            ("write", {"text": text, "claim_key": claim_key, "replaces": replaces,
                       "expected_version": expected_version} | fields)
        )  # fmt: skip
        if replaces is None:
            claim_key = claim_key or f"auto-{next(_ids):016x}"
            live = [
                r
                for r in self.rows
                if r["claim_key"] == claim_key and r["state"] in ("held", "promoted")
            ]
            if live:
                raise refusal(409, "submission.claim_key_conflict")
            versions = [r["version"] for r in self.rows if r["claim_key"] == claim_key]
            row = self.seed(
                claim_key, text, state="held", author=self.key, version=max(versions, default=0) + 1
            )
        else:
            target = next(r for r in self.rows if r["artifact_id"] == replaces)
            if target["state"] != "promoted":
                raise refusal(409, "submission.tip_conflict")
            readable = [
                r["version"] for r in self.rows
                if r["claim_key"] == target["claim_key"]
                and (r["state"] == "promoted" or r["author"] == self.key)
            ]  # fmt: skip
            if expected_version != max(readable):
                raise refusal(409, "submission.version_conflict")
            versions = [r["version"] for r in self.rows if r["claim_key"] == target["claim_key"]]
            row = self.seed(
                target["claim_key"], text, state="held", author=self.key, version=max(versions) + 1
            )
        value = SimpleNamespace(
            artifact_id=row["artifact_id"], claim_key=row["claim_key"], state="held"
        )
        return SimpleNamespace(value=value, idempotency_key="idem", replayed=False)

    def withdraw(self, artifact_id, *, reason=None, idempotency_key=None):
        self.calls.append(("withdraw", {"artifact_id": artifact_id}))
        row = next(r for r in self.rows if r["artifact_id"] == artifact_id)
        if row["author"] != self.key:
            raise refusal(403, "access.author_only")
        if row["state"] != "held":
            raise refusal(409, "memory.not_held")
        row["state"] = "withdrawn"
        return SimpleNamespace(value=SimpleNamespace(state="withdrawn"))

    def verbs(self) -> list[str]:
        return [name for name, _ in self.calls if name != "workspace"]
