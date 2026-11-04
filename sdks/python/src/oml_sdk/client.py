"""The hand-written wrapper over the generated layer (docs/engineering/sdks.md).

It adds what a generator cannot know: where the URL and key come from, one
Idempotency-Key per write that every retry of the call reuses, cursor
iteration, the one error type every refusal raises, and the seven verbs of
ADR-0039 d10 — `write`, `read`, `workarea`, `events`, `review_next`, `approve`
and `reject` — whose fields choose the route as `oml_write`'s and `oml_read`'s
choose theirs. Every request still goes through a generated operation, with
generated models.
"""

from __future__ import annotations

import dataclasses
import datetime
import importlib.metadata
import os
import time
import uuid
from collections.abc import Callable, Iterator, Mapping, Sequence
from typing import Any, Generic, Self, TypeVar

import httpx

from .errors import ApiError, ConfigError, TransportError, error_from_response, refused_locally
from .generated.api.events import tail_events
from .generated.api.keys import whoami
from .generated.api.memories import (
    get_memory,
    list_memories,
    recall_memories,
    submit_memory,
    supersede_memory,
)
from .generated.api.review import post_verdict, review_next
from .generated.api.workareas import get_workarea
from .generated.client import Client
from .generated.models import (
    EventsPage,
    ListMemoriesKind,
    ListMemoriesState,
    Memory,
    MemoryPage,
    MemorySummary,
    RecallRequest,
    RecallRequestKindsItem,
    RecallResponse,
    Serve,
    SubmitAccepted,
    SubmitRequest,
    SubmitRequestKind,
    SupersedeRequest,
    SupersedeRequestKind,
    VerdictRequest,
    VerdictRequestVerdict,
    VerdictResult,
    WorkareaView,
)
from .generated.types import UNSET, Response

T = TypeVar("T")

# What every request sends as User-Agent: the server's request log names the
# client by it (ADR-0042 d9).
USER_AGENT = f"oml-sdk-python/{importlib.metadata.version('oml-sdk')}"

RETRY_AFTER_CAP = 30.0  # seconds; the most a Retry-After is waited for
FIRST_BACKOFF = 0.2  # seconds, doubling per attempt, when no Retry-After is sent

# The rules errors.md states for the two codes a refusal raised before sending
# carries, as the server's refusal of the same arguments would.
VALIDATION_RULE = "The contract is openapi.yaml; what it does not admit is refused, not coerced."
UNRESOLVED_RULE = (
    "The target workarea comes from the request's workarea or, omitted, from the key's "
    "default_workarea. A key with neither is refused, never guessed for."
)

# The arguments of `read`'s recall and listing modes: oml_read's (ADR-0039
# d4), and include_text, which the HTTP recall serves beside them.
READ_RECALL = ("workarea", "k", "kinds", "include_text", "include_unreviewed", "max_tokens")
READ_LISTING = (
    "own",
    "workarea",
    "claim_key",
    "state",
    "kind",
    "since",
    "until",
    "limit",
    "cursor",
)


@dataclasses.dataclass(frozen=True)
class Written(Generic[T]):
    """A write's result, the Idempotency-Key every attempt of it sent, and
    whether the server answered with its stored response (Idempotency-Replay)."""

    value: T
    idempotency_key: str
    replayed: bool


class _Bearer(httpx.Auth):
    """Adds the key to each request and keeps it out of every repr."""

    def __init__(self, key: str) -> None:
        self.__key = key

    def auth_flow(self, request: httpx.Request):
        request.headers["Authorization"] = f"Bearer {self.__key}"
        yield request

    def __repr__(self) -> str:
        return "_Bearer(<key withheld>)"


def _raise_for_refusal(response: httpx.Response) -> None:
    """An httpx response hook: a non-2xx answer becomes its typed exception
    before the generated layer parses it."""
    if response.status_code >= 400:
        response.read()
        raise error_from_response(response.status_code, response.content, response.headers)


def _opt(value: Any) -> Any:
    return UNSET if value is None else value


def _replayed(response: Response[Any]) -> bool:
    return response.headers.get("idempotency-replay", "").lower() == "true"


def _invalid(field: str, message: str) -> ApiError:
    """`request.validation` naming `field`, raised before anything is sent."""
    return refused_locally(
        400, "request.validation", message, VALIDATION_RULE, {"field": f"/{field}"}
    )


def _outside_mode(sent: Mapping[str, Any], allowed: Sequence[str], mode: str) -> None:
    """Refuse, before sending, the first argument set outside `mode`'s."""
    for field, value in sent.items():
        if value is not None and field not in allowed:
            raise _invalid(
                field,
                f"{field} is not an argument of read's {mode} mode: "
                "artifact_id fetches, query recalls, and neither lists",
            )


def _start_pair(url: str | None, api_key: str | None) -> tuple[str, str]:
    """The URL-and-key pair from the first source that sets either value: the
    arguments, then OML_URL and OML_API_KEY. A source setting one alone is
    refused naming the other, so a key is never sent to a URL from another
    source (ADR-0039 d15, the CLI's rule)."""
    sources = (
        ("url", "api_key", "passed", url or None, api_key or None),
        ("OML_URL", "OML_API_KEY", "set", os.environ.get("OML_URL") or None,
         os.environ.get("OML_API_KEY") or None),
    )  # fmt: skip
    for url_name, key_name, verb, found_url, found_key in sources:
        if found_url and found_key:
            return found_url, found_key
        if found_url:
            raise ConfigError(f"{url_name} is {verb} without {key_name}")
        if found_key:
            raise ConfigError(f"{key_name} is {verb} without {url_name}")
    raise ConfigError("no URL and API key: pass url and api_key, or set OML_URL and OML_API_KEY")


class OmlClient:
    """A client for one OML server under one key.

    It starts from one pair: `url` and `api_key`, else `OML_URL` and
    `OML_API_KEY`, taken from the first source that sets either. A source
    setting one alone, or no source at all, raises `ConfigError` before
    anything is sent (ADR-0039 d15).
    """

    def __init__(
        self,
        url: str | None = None,
        api_key: str | None = None,
        *,
        retries: int = 2,
        timeout: float = 10.0,
        transport: httpx.BaseTransport | None = None,
    ) -> None:
        url, api_key = _start_pair(url, api_key)
        if retries < 0:
            raise ConfigError("retries must be 0 or more")
        self._url = url.rstrip("/")
        self._retries = retries
        http = httpx.Client(
            base_url=self._url,
            auth=_Bearer(api_key),
            headers={"User-Agent": USER_AGENT},
            timeout=timeout,
            transport=transport,
            event_hooks={"response": [_raise_for_refusal]},
        )
        self._api = Client(base_url=self._url).set_httpx_client(http)

    def __repr__(self) -> str:
        return f"OmlClient(url={self._url!r})"

    def close(self) -> None:
        self._api.get_httpx_client().close()

    def __enter__(self) -> Self:
        return self

    def __exit__(self, *exc: object) -> None:
        self.close()

    # -- the retry loop ---------------------------------------------------

    def _send(self, call: Callable[[], Response[Any]]) -> Response[Any]:
        """Send one call, retrying transport failures and retryable refusals.
        The caller closes over everything the call sends, its
        Idempotency-Key included, so every attempt sends the same request."""
        attempt = 0
        while True:
            try:
                return call()
            except ApiError as refusal:
                if not refusal.retryable or attempt >= self._retries:
                    raise
                wait = refusal.retry_after
            except httpx.TransportError as failure:
                if attempt >= self._retries:
                    raise TransportError(f"the request failed in transit: {failure}") from failure
                wait = None
            time.sleep(
                min(wait, RETRY_AFTER_CAP) if wait is not None else FIRST_BACKOFF * 2**attempt
            )
            attempt += 1

    def _write(
        self, idempotency_key: str | None, call: Callable[[str], Response[Any]]
    ) -> Written[Any]:
        key = idempotency_key or str(uuid.uuid4())
        response = self._send(lambda: call(key))
        return Written(value=response.parsed, idempotency_key=key, replayed=_replayed(response))

    # -- the provider ring (/v1) --------------------------------------------

    def write(
        self,
        text: str,
        *,
        claim_key: str | None = None,
        kind: str | None = None,
        derives_from: Sequence[str] | None = None,
        workarea: str | None = None,
        replaces: str | None = None,
        retirement: bool | None = None,
        reason: str | None = None,
        idempotency_key: str | None = None,
    ) -> Written[SubmitAccepted]:
        """Write a claim, held on arrival; the fields set choose the route
        (ADR-0039 d3). Without `replaces`, one `POST /v1/memories`, the
        server deriving the verb from `derives_from` and, omitted, the kind
        and the claim key. With `replaces`, a successor of that served tip,
        or with `retirement=True` a proposal to retire it, `text` saying why.
        `retirement` or `reason` without `replaces`, and `claim_key` with it,
        raise `request.validation` naming the field before anything is sent.
        """
        if replaces is None:
            for field, value in (("retirement", retirement), ("reason", reason)):
                if value is not None:
                    raise _invalid(
                        field,
                        f"{field} is refused without replaces: it belongs to a supersession, "
                        "so pass replaces naming the version it changes",
                    )
            submission = SubmitRequest(
                body=text,
                claim_key=_opt(claim_key),
                kind=SubmitRequestKind(kind) if kind is not None else UNSET,
                workarea=_opt(workarea),
                derives_from=list(derives_from) if derives_from is not None else UNSET,
            )
            return self._write(
                idempotency_key,
                lambda key: submit_memory.sync_detailed(
                    client=self._api, body=submission, idempotency_key=key
                ),
            )
        if claim_key is not None:
            raise _invalid(
                "claim_key",
                "claim_key is refused with replaces: the new version takes its target's claim_key",
            )
        successor = SupersedeRequest(
            body=text,
            workarea=_opt(workarea),
            retirement=_opt(retirement),
            kind=SupersedeRequestKind(kind) if kind is not None else UNSET,
            derives_from=list(derives_from) if derives_from is not None else UNSET,
            reason=_opt(reason),
        )
        return self._write(
            idempotency_key,
            lambda key: supersede_memory.sync_detailed(
                replaces, client=self._api, body=successor, idempotency_key=key
            ),
        )

    def read(
        self,
        artifact_id: str | None = None,
        *,
        query: str | None = None,
        workarea: str | None = None,
        k: int | None = None,
        kinds: Sequence[str] | None = None,
        include_text: bool | None = None,
        include_unreviewed: bool | None = None,
        max_tokens: int | None = None,
        own: bool | None = None,
        claim_key: str | None = None,
        state: str | None = None,
        kind: str | None = None,
        since: datetime.datetime | None = None,
        until: datetime.datetime | None = None,
        limit: int | None = None,
        cursor: str | None = None,
    ) -> Memory | RecallResponse | MemoryPage:
        """Read in `oml_read`'s three modes (ADR-0039 d4). `artifact_id`
        fetches one version with its text, a `Memory`. `query` recalls over
        promoted rows in `workarea`, else the key's default workarea, and the
        caller's own held rows with `include_unreviewed`, a `RecallResponse`.
        Neither lists one page, newest first, of the key's own rows, or with
        `own=False` the promoted tips of `workarea`'s subtree, a `MemoryPage`.
        An argument outside the mode, and `query` beside `artifact_id`, raise
        `request.validation` naming it before anything is sent; only the
        fields given are sent."""
        recall = {
            "workarea": workarea,
            "k": k,
            "kinds": kinds,
            "include_text": include_text,
            "include_unreviewed": include_unreviewed,
            "max_tokens": max_tokens,
        }
        listing = {
            "own": own,
            "claim_key": claim_key,
            "state": state,
            "kind": kind,
            "since": since,
            "until": until,
            "limit": limit,
            "cursor": cursor,
        }
        if artifact_id is not None:
            if query is not None:
                raise _invalid(
                    "query",
                    "query is refused with artifact_id: artifact_id fetches one version and "
                    "query recalls, so pass one",
                )
            _outside_mode(recall | listing, (), "get")
            return self._send(
                lambda: get_memory.sync_detailed(artifact_id, client=self._api)
            ).parsed
        if query is not None:
            _outside_mode(listing, READ_RECALL, "recall")
            request = RecallRequest(
                query=query,
                workarea=_opt(workarea),
                k=_opt(k),
                kinds=[RecallRequestKindsItem(kind) for kind in kinds]
                if kinds is not None
                else UNSET,
                include_text=_opt(include_text),
                include_unreviewed=_opt(include_unreviewed),
                max_tokens=_opt(max_tokens),
            )
            return self._send(
                lambda: recall_memories.sync_detailed(client=self._api, body=request)
            ).parsed
        _outside_mode(recall, READ_LISTING, "listing")
        return self._listing(workarea=workarea, **listing)

    def _listing(
        self,
        *,
        own: bool | None = None,
        workarea: str | None = None,
        claim_key: str | None = None,
        state: str | None = None,
        kind: str | None = None,
        since: datetime.datetime | None = None,
        until: datetime.datetime | None = None,
        limit: int | None = None,
        cursor: str | None = None,
    ) -> MemoryPage:
        """One page of `GET /v1/memories`, `own` defaulting to true as
        `oml_read`'s listing does (ADR-0039 d4)."""
        return self._send(
            lambda: list_memories.sync_detailed(
                client=self._api,
                own=True if own is None else own,
                claim_key=_opt(claim_key),
                state=ListMemoriesState(state) if state is not None else UNSET,
                kind=ListMemoriesKind(kind) if kind is not None else UNSET,
                workarea=_opt(workarea),
                since=_opt(since),
                until=_opt(until),
                limit=_opt(limit),
                cursor=_opt(cursor),
            )
        ).parsed

    def pages(self, **filters: Any) -> Iterator[MemoryPage]:
        """Every page of `read`'s listing, following `next_cursor` until it is
        null, with the same filters on every page. Cursors are positions, not
        snapshots: a row that changes between pages can be missed or repeated."""
        if "cursor" in filters:
            raise TypeError("pages() follows the cursor itself; pass filters only")
        cursor = None
        while True:
            page = self._listing(**filters, cursor=cursor)
            yield page
            if not page.next_cursor:
                return
            cursor = page.next_cursor

    def iter_memories(self, **filters: Any) -> Iterator[MemorySummary]:
        """Every row of `read`'s listing, across pages; see `pages`."""
        for page in self.pages(**filters):
            yield from page.items

    # -- workareas and the journal (/v2) -------------------------------------

    def workarea(self, workarea: str | None = None) -> WorkareaView:
        """One workarea node, by id or name, with this key's view of it: its
        children, edges, effective verbs and permission epoch. Omitted, the
        key's default workarea, which `GET /v2/whoami` names; a key with none
        raises `request.workarea_unresolved`."""
        if workarea is None:
            me = self._send(lambda: whoami.sync_detailed(client=self._api)).parsed
            if me.default_workarea is None:
                raise refused_locally(
                    422,
                    "request.workarea_unresolved",
                    "no workarea was named and this key has no default_workarea",
                    UNRESOLVED_RULE,
                    None,
                )
            workarea = me.default_workarea.id
        node = workarea
        return self._send(lambda: get_workarea.sync_detailed(node, client=self._api)).parsed

    def events(
        self,
        *,
        after_seq: int | None = None,
        workarea: str | None = None,
        limit: int | None = None,
    ) -> EventsPage:
        """One page of the journal tail of `workarea`'s subtree, else the key's
        default workarea's, in seq order after `after_seq`; pass the page's
        `next_after_seq` back to resume. Digests only, never claim text."""
        return self._send(
            lambda: tail_events.sync_detailed(
                client=self._api,
                after_seq=_opt(after_seq),
                workarea=_opt(workarea),
                limit=_opt(limit),
            )
        ).parsed

    # -- the review ceremony (/v2), for a dreamer key ------------------------

    def review_next(self) -> Serve | None:
        """Serve exactly one held item to this reviewer, or None when nothing waits."""
        key = str(uuid.uuid4())
        response = self._send(
            lambda: review_next.sync_detailed(client=self._api, idempotency_key=key)
        )
        return response.parsed if response.status_code == 200 else None

    def approve(
        self,
        serve_id: str | None = None,
        *,
        reason: str | None = None,
        idempotency_key: str | None = None,
    ) -> Written[VerdictResult]:
        """Approve the `serve_id` that `review_next` returned, or, omitted, the
        one item this key has claimed (ADR-0039 d10); the id is opaque and
        never built here."""
        return self._verdict(VerdictRequestVerdict.APPROVE, serve_id, reason, idempotency_key)

    def reject(
        self,
        serve_id: str | None = None,
        *,
        reason: str,
        idempotency_key: str | None = None,
    ) -> Written[VerdictResult]:
        """Reject the `serve_id` that `review_next` returned, or, omitted, the
        one item this key has claimed, with the reason a rejection requires."""
        return self._verdict(VerdictRequestVerdict.REJECT, serve_id, reason, idempotency_key)

    def _verdict(
        self,
        verdict: VerdictRequestVerdict,
        serve_id: str | None,
        reason: str | None,
        idempotency_key: str | None,
    ) -> Written[VerdictResult]:
        request = VerdictRequest(verdict=verdict, serve_id=_opt(serve_id), reason=_opt(reason))
        return self._write(
            idempotency_key,
            lambda key: post_verdict.sync_detailed(
                client=self._api, body=request, idempotency_key=key
            ),
        )
