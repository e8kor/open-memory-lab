"""`OmlStore`: a LangGraph `BaseStore` over one in-progress workspace (ADR-0094 d4-d9).

An in-progress workspace serves every author's held rows to the keys inside it
(ADR-0030 d3), so a value put here is read back at once by every agent of the
crew, and nothing leaves the workspace until a person reviews all of it and
commits (ADR-0030 d6). Anywhere else a put would be invisible to every other key
until review, so the store refuses any other workspace by name.

The mapping (ADR-0094 d5-d8):

- an item is the claim `store/<label>/.../<key>`, each part percent-encoded,
  in the store's workspace; its value is the version's JSON body;
- `put` writes a claim, or supersedes the approved tip, after withdrawing the
  key's own held version of the claim;
- `get` and `search` read held and approved rows, the newest version of each
  claim; a claim whose approved tip is a retirement is absent;
- `delete` withdraws the key's own held version, and refuses anything else.
"""

from __future__ import annotations

import asyncio
import datetime
import json
import urllib.parse
from collections.abc import Iterable, Mapping, Sequence
from typing import Any

from langgraph.store.base import (
    BaseStore,
    GetOp,
    Item,
    ListNamespacesOp,
    Op,
    PutOp,
    Result,
    SearchItem,
    SearchOp,
    validate_op_namespace,
)

from ..client import OmlClient
from ..errors import ApiError, OmlError
from ..generated.models import Memory
from ..generated.types import Unset

STORE_PREFIX = "store/"
"""The claim-key prefix every item of an `OmlStore` lives under (ADR-0094 d5)."""

JSON = "application/json"
RECALL_K_MAX = 100  # RecallRequest.k's maximum in openapi.yaml
PAGE = 100  # the listing's page size the store asks for


class StoreRefused(OmlError):
    """An `OmlStore` operation the store refuses before or instead of a
    request, naming why in `reason`:

    - `not_in_progress`: the workspace is not inside an in-progress subtree;
    - `not_found`: a delete of a key with no version the key reads;
    - `approved`: a delete of a key whose claim has an approved version, which
      only a reviewed retirement takes out of service;
    - `not_author`: a put or a delete of a key another key wrote; only the
      key that wrote an item updates it;
    - `empty_key`: a put or a delete of the empty key;
    - `limit_exceeded`: a search with a query whose `offset + limit` passes
      recall's cap of 100 hits;
    - `not_json`: a stored version whose text is not a JSON object;
    - `unsupported`: an argument the store does not implement, such as a TTL.
    """

    def __init__(self, reason: str, message: str) -> None:
        super().__init__(f"{reason}: {message}")
        self.reason = reason
        self.message = message


def _part(label: str) -> str:
    return urllib.parse.quote(label, safe="")


def claim_key_for(namespace: Sequence[str], key: str) -> str:
    """The claim key of `key` in `namespace` (ADR-0094 d5)."""
    return STORE_PREFIX + "/".join(_part(part) for part in (*namespace, key))


def _prefix_for(namespace: Sequence[str]) -> str:
    """The claim-key prefix of every item at or below `namespace`."""
    return STORE_PREFIX + "".join(_part(label) + "/" for label in namespace)


def _parse(claim_key: str) -> tuple[tuple[str, ...], str] | None:
    """`claim_key_for`'s inverse; None for a key outside the store."""
    if not claim_key.startswith(STORE_PREFIX):
        return None
    parts = [urllib.parse.unquote(p) for p in claim_key[len(STORE_PREFIX) :].split("/")]
    if len(parts) < 2 or not all(parts):
        return None
    return tuple(parts[:-1]), parts[-1]


def _state(row: Memory) -> str:
    return str(row.state)


def _newest(rows: Iterable[Memory]) -> dict[str, Memory]:
    """The newest served version of each claim key, without the claims whose
    newest is an approved retirement, which hold no value
    (audit-and-lineage.md: once approved, a retirement serves nothing of the
    claim). A held retirement is a proposal, not a value, and is passed over."""
    newest: dict[str, Memory] = {}
    for row in rows:
        state = _state(row)
        if state not in ("held", "promoted") or (state == "held" and row.retirement):
            continue
        seen = newest.get(row.claim_key)
        if seen is None or row.version > seen.version:
            newest[row.claim_key] = row
    return {key: row for key, row in newest.items() if not row.retirement}


def _approved_tip(rows: Iterable[Memory]) -> Memory | None:
    """The claim's approved tip the listing serves, retirement or not."""
    return next((r for r in rows if _state(r) == "promoted"), None)


def _value_of(row: Memory) -> dict[str, Any]:
    text = row.text if not isinstance(row.text, Unset) else None
    try:
        value = json.loads(text) if text is not None else None
    except json.JSONDecodeError:
        value = None
    if not isinstance(value, dict):
        raise StoreRefused(
            "not_json", f"{row.claim_key} v{row.version} ({row.artifact_id}) is not a JSON object"
        )
    return value


_COMPARE = {
    "$eq": lambda a, b: a == b,
    "$ne": lambda a, b: a != b,
    "$gt": lambda a, b: a > b,
    "$gte": lambda a, b: a >= b,
    "$lt": lambda a, b: a < b,
    "$lte": lambda a, b: a <= b,
}


def _matches(value: Mapping[str, Any], filter: Mapping[str, Any] | None) -> bool:
    """LangGraph's filter: each top-level field equal to the value given, or
    meeting each `$eq`/`$ne`/`$gt`/`$gte`/`$lt`/`$lte` of an operator mapping;
    a nested value compares by exact equality, and any other `$` operator
    raises `ValueError`."""
    for field, wanted in (filter or {}).items():
        have = value.get(field)
        ops = wanted if isinstance(wanted, Mapping) else ()
        unknown = [op for op in ops if str(op).startswith("$") and op not in _COMPARE]
        if unknown:
            raise ValueError(
                f"OmlStore's filter knows {', '.join(_COMPARE)}; not {', '.join(unknown)}"
            )
        if isinstance(wanted, Mapping) and wanted and all(op in _COMPARE for op in wanted):
            try:
                if not all(_COMPARE[op](have, operand) for op, operand in wanted.items()):
                    return False
            except TypeError:
                return False
        elif have != wanted:
            return False
    return True


AUTHOR_CODES = ("access.author_only", "submission.claim_key_conflict")
"""The server's refusals of a put over another key's claim: a supersession of
a claim the key did not write, and a first version over another's held one."""


def _not_author(claim_key: str, artifact_id: str | None) -> StoreRefused:
    held = f" ({artifact_id})" if artifact_id else ""
    return StoreRefused(
        "not_author",
        f"{claim_key} was written by another key{held}; only the key that wrote an item"
        " updates or deletes it",
    )


class OmlStore(BaseStore):
    """A LangGraph `BaseStore` over one in-progress workspace (ADR-0094).

    `workspace` names it by id or name, else the key's default workspace is
    used. Construction reads the workspace and raises `StoreRefused`
    (`not_in_progress`) unless it lies inside an in-progress subtree; every
    batch that writes reads it again, so a store whose workspace was committed
    meanwhile refuses the write rather than leave it held where no other key
    reads it.

    Every value is held until a reviewer judges it, and the workspace commits
    only once none is held (ADR-0030 d6): each put is a version a person or a
    reviewer crew reviews before the work leaves the workspace.
    """

    supports_ttl = False

    def __init__(self, client: OmlClient, workspace: str | None = None) -> None:
        self._client = client
        view = client.workspace(workspace)
        self._workspace = view.id
        self._name = view.name
        self._require_in_progress(view)

    @property
    def workspace(self) -> str:
        """The workspace's id."""
        return self._workspace

    def __repr__(self) -> str:
        return f"OmlStore(workspace={self._name!r})"

    # -- the mode check -----------------------------------------------------

    def _require_in_progress(self, view: Any = None) -> None:
        view = view if view is not None else self._client.workspace(self._workspace)
        root = view.in_progress_root
        if root is None or isinstance(root, Unset):
            raise StoreRefused(
                "not_in_progress",
                f"workspace {view.name} is {view.mode if not isinstance(view.mode, Unset) else 'standard'}"
                " and lies inside no in-progress workspace; OmlStore writes only where every"
                " reader of the workspace reads held rows (ADR-0030, ADR-0094 d4)",
            )

    # -- reads ----------------------------------------------------------------

    def _catalog(self, *, claim_key: str | None = None, prefix: str | None = None) -> list[Memory]:
        """Every row the scoped listing serves: approved tips and, inside the
        in-progress subtree, every author's held rows (ADR-0030 d3)."""
        return list(
            self._client.iter_memories(
                workspace=self._workspace,
                claim_key=claim_key,
                prefix=prefix,
                include_text=True,
                limit=PAGE,
            )
        )

    def _own(self, claim_key: str) -> list[Memory]:
        """The key's own versions of `claim_key` here, in every state; the
        server reads the author off the key, never off the request."""
        return list(
            self._client.iter_memories(
                own=True, workspace=self._workspace, claim_key=claim_key, limit=PAGE
            )
        )

    def _item(
        self, row: Memory, created: datetime.datetime | None = None, cls: Any = Item, **extra
    ):
        namespace, key = _parse(row.claim_key)  # type: ignore[misc]
        return cls(
            namespace=namespace,
            key=key,
            value=_value_of(row),
            created_at=created or row.created_at,
            updated_at=row.created_at,
            **extra,
        )

    def _get(self, op: GetOp) -> Item | None:
        claim_key = claim_key_for(op.namespace, op.key)
        rows = self._catalog(claim_key=claim_key)
        row = _newest(rows).get(claim_key)
        if row is None:
            return None
        first = min((r.created_at for r in rows), default=row.created_at)
        return self._item(row, first)

    def _search(self, op: SearchOp) -> list[SearchItem]:
        prefix = _prefix_for(op.namespace_prefix)
        if op.query:
            if op.offset + op.limit > RECALL_K_MAX:
                raise StoreRefused(
                    "limit_exceeded",
                    f"a search with a query reads at most {RECALL_K_MAX} recall hits;"
                    f" offset {op.offset} + limit {op.limit} passes that",
                )
            answer = self._client.read(
                query=op.query,
                workspace=self._workspace,
                k=RECALL_K_MAX,
                include_text=True,
                include_unreviewed=True,
            )
            scored = [
                (hit.memory, hit.score)
                for hit in answer.hits
                if hit.memory.claim_key.startswith(prefix) and _parse(hit.memory.claim_key)
            ]
            # A hit is served only when it is its claim's newest version: the
            # score is the hit's own, so a superseded version's match is
            # dropped rather than lend its score to text it does not hold.
            newest: dict[str, Memory | None] = {}
            for memory, _ in scored:
                if memory.claim_key not in newest:
                    newest[memory.claim_key] = _newest(
                        self._catalog(claim_key=memory.claim_key)
                    ).get(memory.claim_key)
            ranked = [
                (memory, score)
                for memory, score in scored
                if (tip := newest[memory.claim_key]) is not None
                and tip.artifact_id == memory.artifact_id
            ]
        else:
            rows = [r for r in self._catalog(prefix=prefix) if _parse(r.claim_key)]
            newest = _newest(rows)
            ranked = [(row, None) for row in sorted(newest.values(), key=lambda r: r.claim_key)]
        found = []
        for row, score in ranked:
            item = self._item(row, cls=SearchItem, score=score)
            if _matches(item.value, op.filter):
                found.append(item)
        return found[op.offset : op.offset + op.limit]

    def _list_namespaces(self, op: ListNamespacesOp) -> list[tuple[str, ...]]:
        spaces = set()
        for row in _newest(self._catalog(prefix=STORE_PREFIX)).values():
            parsed = _parse(row.claim_key)
            if parsed is None:
                continue
            namespace = parsed[0]
            if all(self._condition(namespace, c) for c in op.match_conditions or ()):
                spaces.add(namespace[: op.max_depth] if op.max_depth is not None else namespace)
        return sorted(spaces)[op.offset : op.offset + op.limit]

    @staticmethod
    def _condition(namespace: tuple[str, ...], condition: Any) -> bool:
        path = tuple(condition.path)
        if not path:
            return True
        if len(path) > len(namespace):
            return False
        part = (
            namespace[: len(path)] if condition.match_type == "prefix" else namespace[-len(path) :]
        )
        return all(want in ("*", have) for want, have in zip(path, part, strict=True))

    # -- writes ---------------------------------------------------------------

    def _put(self, op: PutOp) -> None:
        if op.ttl is not None:
            raise StoreRefused("unsupported", "OmlStore keeps no TTL: a version is never deleted")
        if op.key == "":
            raise StoreRefused("empty_key", "an item's key names its claim and cannot be empty")
        claim_key = claim_key_for(op.namespace, op.key)
        if op.value is None:
            self._delete(claim_key)
            return
        body = json.dumps(dict(op.value), ensure_ascii=False, sort_keys=True)
        title = "/".join((*op.namespace, op.key))
        rows = self._catalog(claim_key=claim_key)
        own = self._own(claim_key)
        own_held = [r for r in own if _state(r) == "held"]
        own_ids = {r.artifact_id for r in own}
        others = [r for r in rows if _state(r) == "held" and r.artifact_id not in own_ids]
        if others:
            raise _not_author(claim_key, others[0].artifact_id)
        tip = _approved_tip(rows)
        try:
            if tip is None:
                # A new first version takes the claim key only once the key's
                # own held one is withdrawn (audit-and-lineage.md: a withdrawn
                # first version releases the key).
                self._withdraw(own_held)
                self._client.write(
                    body,
                    claim_key=claim_key,
                    workspace=self._workspace,
                    format=JSON,
                    title=title,
                )
                return
            # The latest version the key reads: the approved ones and its own,
            # in any state; never another author's held one (ADR-0057 d19). A
            # retirement tip is superseded like any other, which returns the
            # claim to service once approved.
            expected = max(r.version for r in [tip, *own])
            self._client.write(
                body,
                replaces=tip.artifact_id,
                expected_version=expected,
                workspace=self._workspace,
                format=JSON,
                title=title,
            )
        except ApiError as refused:
            # A key supersedes only its own claims (`access.author_only`), and
            # another key's first version may land between the read and this
            # write (`submission.claim_key_conflict`).
            if refused.code in AUTHOR_CODES:
                raise _not_author(claim_key, None) from refused
            raise
        self._withdraw(own_held)

    def _withdraw(self, rows: Iterable[Memory]) -> None:
        for row in rows:
            self._client.withdraw(row.artifact_id, reason="replaced or deleted through OmlStore")

    def _delete(self, claim_key: str) -> None:
        rows = self._catalog(claim_key=claim_key)
        own = {r.artifact_id: r for r in self._own(claim_key)}
        tip = _approved_tip(rows)
        if tip is not None and not tip.retirement:
            raise StoreRefused(
                "approved",
                f"{claim_key} has an approved version; only a reviewed retirement takes it out of"
                " service, and the store proposes none",
            )
        others = [r for r in rows if _state(r) == "held" and r.artifact_id not in own]
        if others:
            raise _not_author(claim_key, others[0].artifact_id)
        held = [r for r in own.values() if _state(r) == "held"]
        if not held:
            raise StoreRefused("not_found", f"{claim_key} has no version this key reads")
        self._withdraw(held)

    # -- BaseStore ------------------------------------------------------------

    def batch(self, ops: Iterable[Op]) -> list[Result]:
        ops = list(ops)
        for op in ops:
            validate_op_namespace(op)
        if any(isinstance(op, PutOp) for op in ops):
            self._require_in_progress()
        results: list[Result] = []
        for op in ops:
            if isinstance(op, GetOp):
                results.append(self._get(op))
            elif isinstance(op, SearchOp):
                results.append(self._search(op))
            elif isinstance(op, ListNamespacesOp):
                results.append(self._list_namespaces(op))
            elif isinstance(op, PutOp):
                results.append(self._put(op))
            else:
                raise StoreRefused("unsupported", f"unknown operation {type(op).__name__}")
        return results

    async def abatch(self, ops: Iterable[Op]) -> list[Result]:
        """`batch` in a worker thread: the client is synchronous."""
        return await asyncio.to_thread(self.batch, list(ops))
