"""The hand-written wrapper over the generated layer (docs/engineering/sdks.md).

It adds what a generator cannot know: where the key comes from, one
Idempotency-Key per write that every retry of the call reuses, cursor
iteration, typed exceptions per errors.md family, and the upload stub. Every
request still goes through a generated operation, with generated models.
"""

from __future__ import annotations

import dataclasses
import datetime
import os
import time
import uuid
from collections.abc import Callable, Iterator, Sequence
from typing import Any, Generic, NoReturn, Self, TypeVar

import httpx

from .errors import ApiError, ConfigError, NotBuiltError, TransportError, error_from_response
from .generated.api.memories import (
    get_memory,
    propose_supersession,
    recall_memories,
    search_memories,
    submit_memory,
)
from .generated.api.review import post_verdict, review_next
from .generated.client import Client
from .generated.models import (
    Memory,
    MemoryPage,
    MemorySummary,
    RecallRequest,
    RecallRequestKindsItem,
    RecallResponse,
    SearchMemoriesKind,
    SearchMemoriesState,
    Serve,
    SubmitAccepted,
    SubmitRequest,
    SubmitRequestKind,
    SubmitRequestVerb,
    SupersedeRequest,
    SupersedeRequestKind,
    VerdictRequest,
    VerdictRequestVerdict,
    VerdictResult,
)
from .generated.types import UNSET, Response

T = TypeVar("T")

RETRY_AFTER_CAP = 30.0  # seconds; the most a Retry-After is waited for
FIRST_BACKOFF = 0.2  # seconds, doubling per attempt, when no Retry-After is sent


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


class OmlClient:
    """A client for one OML server under one key.

    The key is `api_key`, else `OML_API_KEY`; the server is `base_url`, else
    `OML_URL`. Either missing raises `ConfigError` before anything is sent.
    """

    def __init__(
        self,
        base_url: str | None = None,
        api_key: str | None = None,
        *,
        retries: int = 2,
        timeout: float = 10.0,
        transport: httpx.BaseTransport | None = None,
    ) -> None:
        base_url = base_url or os.environ.get("OML_URL")
        api_key = api_key or os.environ.get("OML_API_KEY")
        if not base_url:
            raise ConfigError("no server: pass base_url or set OML_URL")
        if not api_key:
            raise ConfigError("no key: pass api_key or set OML_API_KEY")
        if retries < 0:
            raise ConfigError("retries must be 0 or more")
        self._base_url = base_url.rstrip("/")
        self._retries = retries
        http = httpx.Client(
            base_url=self._base_url,
            auth=_Bearer(api_key),
            timeout=timeout,
            transport=transport,
            event_hooks={"response": [_raise_for_refusal]},
        )
        self._api = Client(base_url=self._base_url).set_httpx_client(http)

    def __repr__(self) -> str:
        return f"OmlClient(base_url={self._base_url!r})"

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

    def remember(
        self,
        claim_key: str,
        body: str,
        *,
        kind: str = "memory",
        derives_from: Sequence[str] | None = None,
        idempotency_key: str | None = None,
        workarea: str | None = None,
    ) -> Written[SubmitAccepted]:
        """Submit a claim with verb `remember`; it is held on arrival."""
        return self._submit(
            SubmitRequestVerb.REMEMBER,
            claim_key,
            body,
            kind,
            derives_from,
            idempotency_key,
            workarea,
        )

    def learn(
        self,
        claim_key: str,
        body: str,
        *,
        derives_from: Sequence[str],
        kind: str = "derived_fact",
        idempotency_key: str | None = None,
        workarea: str | None = None,
    ) -> Written[SubmitAccepted]:
        """Submit a derived claim with verb `learn`; `derives_from` is its lineage."""
        return self._submit(
            SubmitRequestVerb.LEARN, claim_key, body, kind, derives_from, idempotency_key, workarea
        )

    def _submit(self, verb, claim_key, body, kind, derives_from, idempotency_key, workarea):
        request = SubmitRequest(
            verb=verb,
            claim_key=claim_key,
            kind=SubmitRequestKind(kind),
            body=body,
            derives_from=list(derives_from) if derives_from is not None else UNSET,
        )
        return self._write(
            idempotency_key,
            lambda key: submit_memory.sync_detailed(
                client=self._api, body=request, idempotency_key=key, x_oml_workarea=_opt(workarea)
            ),
        )

    def recall(
        self,
        query: str,
        *,
        k: int | None = None,
        kinds: Sequence[str] | None = None,
        include_bodies: bool | None = None,
        include_own_held: bool | None = None,
        max_tokens: int | None = None,
        workarea: str | None = None,
    ) -> RecallResponse:
        """Ranked recall over promoted rows, and the caller's own held rows
        with `include_own_held`. Only the fields given are sent: the
        generated model would send its schema defaults otherwise."""
        request = RecallRequest(
            query=query,
            k=_opt(k),
            kinds=[RecallRequestKindsItem(kind) for kind in kinds] if kinds is not None else UNSET,
            include_bodies=_opt(include_bodies),
            include_own_held=_opt(include_own_held),
            max_tokens=_opt(max_tokens),
            terms=UNSET,
            expand_terms=UNSET,
        )
        return self._send(
            lambda: recall_memories.sync_detailed(
                client=self._api, body=request, x_oml_workarea=_opt(workarea)
            )
        ).parsed

    def get(self, artifact_id: str) -> Memory:
        """One memory with its body."""
        return self._send(lambda: get_memory.sync_detailed(artifact_id, client=self._api)).parsed

    def list_memories(
        self,
        *,
        own: bool | None = None,
        claim_key: str | None = None,
        state: str | None = None,
        kind: str | None = None,
        workarea_id: str | None = None,
        since: datetime.datetime | None = None,
        until: datetime.datetime | None = None,
        limit: int | None = None,
        cursor: str | None = None,
        workarea: str | None = None,
    ) -> MemoryPage:
        """One page of `GET /v1/memories`, newest first, with its `next_cursor`."""
        return self._send(
            lambda: search_memories.sync_detailed(
                client=self._api,
                own=_opt(own),
                claim_key=_opt(claim_key),
                state=SearchMemoriesState(state) if state is not None else UNSET,
                kind=SearchMemoriesKind(kind) if kind is not None else UNSET,
                workarea_id=_opt(workarea_id),
                since=_opt(since),
                until=_opt(until),
                limit=_opt(limit),
                cursor=_opt(cursor),
                x_oml_workarea=_opt(workarea),
            )
        ).parsed

    def pages(self, **filters: Any) -> Iterator[MemoryPage]:
        """Every page of the listing, following `next_cursor` until it is null,
        with the same filters on every page. Cursors are positions, not
        snapshots: a row that changes between pages can be missed or repeated."""
        if "cursor" in filters:
            raise TypeError("pages() follows the cursor itself; pass filters only")
        cursor = None
        while True:
            page = self.list_memories(**filters, cursor=cursor)
            yield page
            if not page.next_cursor:
                return
            cursor = page.next_cursor

    def iter_memories(self, **filters: Any) -> Iterator[MemorySummary]:
        """Every row of the listing, across pages; see `pages`."""
        for page in self.pages(**filters):
            yield from page.items

    def supersede(
        self,
        artifact_id: str,
        body: str,
        *,
        kind: str | None = None,
        derives_from: Sequence[str] | None = None,
        retirement: bool | None = None,
        idempotency_key: str | None = None,
        workarea: str | None = None,
    ) -> Written[SubmitAccepted]:
        """Propose a successor of the served tip `artifact_id`, held like any
        write; `retirement=True` proposes retiring the claim, `body` saying why."""
        request = SupersedeRequest(
            body=body,
            retirement=_opt(retirement),
            kind=SupersedeRequestKind(kind) if kind is not None else UNSET,
            derives_from=list(derives_from) if derives_from is not None else UNSET,
        )
        return self._write(
            idempotency_key,
            lambda key: propose_supersession.sync_detailed(
                artifact_id,
                client=self._api,
                body=request,
                idempotency_key=key,
                x_oml_workarea=_opt(workarea),
            ),
        )

    def upload_attachment(self, data: bytes, *, media_type: str) -> NoReturn:
        """Not built: the server serves no upload sessions yet, so this raises
        before sending anything (docs/engineering/sdks.md, "Uploads")."""
        raise NotBuiltError(
            "attachment uploads are not built: the server serves no upload sessions yet (FR-12)"
        )

    # -- the review ceremony (/v2), for a dreamer key ------------------------

    def review_next(self) -> Serve | None:
        """Serve exactly one held item to this reviewer, or None when nothing waits."""
        key = str(uuid.uuid4())
        response = self._send(
            lambda: review_next.sync_detailed(client=self._api, idempotency_key=key)
        )
        return response.parsed if response.status_code == 200 else None

    def verdict(
        self,
        serve_id: str,
        verdict: str,
        *,
        reason: str | None = None,
        idempotency_key: str | None = None,
    ) -> Written[VerdictResult]:
        """Record `approve` or `reject` (with a reason) against the `serve_id`
        that `review_next` returned; the id is opaque and never built here."""
        request = VerdictRequest(
            serve_id=serve_id, verdict=VerdictRequestVerdict(verdict), reason=_opt(reason)
        )
        return self._write(
            idempotency_key,
            lambda key: post_verdict.sync_detailed(
                client=self._api, body=request, idempotency_key=key
            ),
        )
