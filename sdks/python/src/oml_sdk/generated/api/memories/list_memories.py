import datetime
from http import HTTPStatus
from typing import Any

import httpx

from ...client import AuthenticatedClient, Client
from ...models.error_envelope import ErrorEnvelope
from ...models.list_memories_kind import ListMemoriesKind
from ...models.list_memories_state import ListMemoriesState
from ...models.memory_page import MemoryPage
from ...models.origin_kind import OriginKind
from ...types import UNSET, Response, Unset


def _get_kwargs(
    *,
    workarea: str | Unset = UNSET,
    claim_key: str | Unset = UNSET,
    state: ListMemoriesState | Unset = UNSET,
    kind: ListMemoriesKind | Unset = UNSET,
    own: bool | Unset = False,
    with_own: bool | Unset = UNSET,
    author: str | Unset = UNSET,
    anchor_repo: str | Unset = UNSET,
    anchor_path_prefix: str | Unset = UNSET,
    origin_kinds: list[OriginKind] | Unset = UNSET,
    since: datetime.datetime | Unset = UNSET,
    until: datetime.datetime | Unset = UNSET,
    limit: int | Unset = 50,
    cursor: str | Unset = UNSET,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> dict[str, Any]:
    headers: dict[str, Any] = {}
    if not isinstance(x_request_id, Unset):
        headers["X-Request-Id"] = x_request_id

    if not isinstance(x_correlation_id, Unset):
        headers["X-Correlation-Id"] = x_correlation_id

    if not isinstance(traceparent, Unset):
        headers["traceparent"] = traceparent

    params: dict[str, Any] = {}

    json_workarea: str | Unset
    if isinstance(workarea, Unset):
        json_workarea = UNSET
    else:
        json_workarea = workarea
    params["workarea"] = json_workarea

    params["claim_key"] = claim_key

    json_state: str | Unset = UNSET
    if not isinstance(state, Unset):
        json_state = state.value

    params["state"] = json_state

    json_kind: str | Unset = UNSET
    if not isinstance(kind, Unset):
        json_kind = kind.value

    params["kind"] = json_kind

    params["own"] = own

    params["with_own"] = with_own

    params["author"] = author

    params["anchor_repo"] = anchor_repo

    params["anchor_path_prefix"] = anchor_path_prefix

    json_origin_kinds: list[str] | Unset = UNSET
    if not isinstance(origin_kinds, Unset):
        json_origin_kinds = []
        for componentsschemas_origin_kinds_item_data in origin_kinds:
            componentsschemas_origin_kinds_item = (
                componentsschemas_origin_kinds_item_data.value
            )
            json_origin_kinds.append(componentsschemas_origin_kinds_item)

    params["origin_kinds"] = json_origin_kinds

    json_since: str | Unset = UNSET
    if not isinstance(since, Unset):
        json_since = since.isoformat()
    params["since"] = json_since

    json_until: str | Unset = UNSET
    if not isinstance(until, Unset):
        json_until = until.isoformat()
    params["until"] = json_until

    params["limit"] = limit

    params["cursor"] = cursor

    params = {k: v for k, v in params.items() if v is not UNSET and v is not None}

    _kwargs: dict[str, Any] = {
        "method": "get",
        "url": "/v1/memories",
        "params": params,
    }

    _kwargs["headers"] = headers
    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> ErrorEnvelope | MemoryPage:
    if response.status_code == 200:
        response_200 = MemoryPage.from_dict(response.json())

        return response_200

    if response.status_code == 429:
        response_429 = ErrorEnvelope.from_dict(response.json())

        return response_429

    response_default = ErrorEnvelope.from_dict(response.json())

    return response_default


def _build_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Response[ErrorEnvelope | MemoryPage]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    *,
    client: AuthenticatedClient | Client,
    workarea: str | Unset = UNSET,
    claim_key: str | Unset = UNSET,
    state: ListMemoriesState | Unset = UNSET,
    kind: ListMemoriesKind | Unset = UNSET,
    own: bool | Unset = False,
    with_own: bool | Unset = UNSET,
    author: str | Unset = UNSET,
    anchor_repo: str | Unset = UNSET,
    anchor_path_prefix: str | Unset = UNSET,
    origin_kinds: list[OriginKind] | Unset = UNSET,
    since: datetime.datetime | Unset = UNSET,
    until: datetime.datetime | Unset = UNSET,
    limit: int | Unset = 50,
    cursor: str | Unset = UNSET,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> Response[ErrorEnvelope | MemoryPage]:
    """Structured, paginated listing of memory summaries.

     Requires: reader effective access; own=true admits any tenant role except operator
    (access.verb_ceiling) for its own rows only — every row the calling key authored, in every state,
    across all workareas minus its globally denied subtrees, no grant needed (ADR-0025 d13). With
    own=true and no workarea it lists every workarea the key wrote in. Without own, the listing returns
    promoted served tips within reader effective access in the workarea's subtree, defaulting to the
    key's default_workarea (ADR-0039 d4) — a self_signed version only when the scope node is its own
    workarea; a retired claim's tip, its retirement version, among them, flagged retirement true;
    others' held rows are never returned (R3). workarea is the subtree read (see the parameter);
    v0.3.0's workarea_id parameter and X-OML-Workarea header are request.validation naming workarea in
    details.renamed_to (ADR-0039 d7). Ordered created_at descending, then artifact_id descending; the
    cursor is bound to the calling key and expires under api.cursor_ttl.

    Args:
        workarea (str | Unset): A workarea, by its node id, its name or its qualified name.
            Resolved server-side before any check that reads state; an unknown name answers exactly
            what an unknown id answers. A user key resolves a bare name in its own user root's
            namespace, plus universe and its root's name; every other key in the shared namespace
            (ADR-0044 d5). A derived path, a value of three or more labels or one whose first label is
            universe, is neither, and is refused (request.validation).
        claim_key (str | Unset):
        state (ListMemoriesState | Unset):
        kind (ListMemoriesKind | Unset):
        own (bool | Unset):  Default: False.
        with_own (bool | Unset):
        author (str | Unset):
        anchor_repo (str | Unset): A repository, scheme, credentials and .git dropped
            (github.com/e8kor/open-memory-lab); no segment is . or .. (ADR-0057 d3). As a filter, rows
            whose anchor names it; unanchored rows are left out (d4).
        anchor_path_prefix (str | Unset): A byte prefix of the anchor path, at most 1,024 UTF-8
            bytes; rows whose anchor path starts with it, unanchored rows left out (ADR-0057 d4).
        origin_kinds (list[OriginKind] | Unset): Rows whose origin kind is one of these, combined
            with every other filter by AND; rows without a kind are left out (ADR-0063 d3).
        since (datetime.datetime | Unset):
        until (datetime.datetime | Unset):
        limit (int | Unset):  Default: 50.
        cursor (str | Unset):
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | MemoryPage]
    """

    kwargs = _get_kwargs(
        workarea=workarea,
        claim_key=claim_key,
        state=state,
        kind=kind,
        own=own,
        with_own=with_own,
        author=author,
        anchor_repo=anchor_repo,
        anchor_path_prefix=anchor_path_prefix,
        origin_kinds=origin_kinds,
        since=since,
        until=until,
        limit=limit,
        cursor=cursor,
        x_request_id=x_request_id,
        x_correlation_id=x_correlation_id,
        traceparent=traceparent,
    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    *,
    client: AuthenticatedClient | Client,
    workarea: str | Unset = UNSET,
    claim_key: str | Unset = UNSET,
    state: ListMemoriesState | Unset = UNSET,
    kind: ListMemoriesKind | Unset = UNSET,
    own: bool | Unset = False,
    with_own: bool | Unset = UNSET,
    author: str | Unset = UNSET,
    anchor_repo: str | Unset = UNSET,
    anchor_path_prefix: str | Unset = UNSET,
    origin_kinds: list[OriginKind] | Unset = UNSET,
    since: datetime.datetime | Unset = UNSET,
    until: datetime.datetime | Unset = UNSET,
    limit: int | Unset = 50,
    cursor: str | Unset = UNSET,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> ErrorEnvelope | MemoryPage | None:
    """Structured, paginated listing of memory summaries.

     Requires: reader effective access; own=true admits any tenant role except operator
    (access.verb_ceiling) for its own rows only — every row the calling key authored, in every state,
    across all workareas minus its globally denied subtrees, no grant needed (ADR-0025 d13). With
    own=true and no workarea it lists every workarea the key wrote in. Without own, the listing returns
    promoted served tips within reader effective access in the workarea's subtree, defaulting to the
    key's default_workarea (ADR-0039 d4) — a self_signed version only when the scope node is its own
    workarea; a retired claim's tip, its retirement version, among them, flagged retirement true;
    others' held rows are never returned (R3). workarea is the subtree read (see the parameter);
    v0.3.0's workarea_id parameter and X-OML-Workarea header are request.validation naming workarea in
    details.renamed_to (ADR-0039 d7). Ordered created_at descending, then artifact_id descending; the
    cursor is bound to the calling key and expires under api.cursor_ttl.

    Args:
        workarea (str | Unset): A workarea, by its node id, its name or its qualified name.
            Resolved server-side before any check that reads state; an unknown name answers exactly
            what an unknown id answers. A user key resolves a bare name in its own user root's
            namespace, plus universe and its root's name; every other key in the shared namespace
            (ADR-0044 d5). A derived path, a value of three or more labels or one whose first label is
            universe, is neither, and is refused (request.validation).
        claim_key (str | Unset):
        state (ListMemoriesState | Unset):
        kind (ListMemoriesKind | Unset):
        own (bool | Unset):  Default: False.
        with_own (bool | Unset):
        author (str | Unset):
        anchor_repo (str | Unset): A repository, scheme, credentials and .git dropped
            (github.com/e8kor/open-memory-lab); no segment is . or .. (ADR-0057 d3). As a filter, rows
            whose anchor names it; unanchored rows are left out (d4).
        anchor_path_prefix (str | Unset): A byte prefix of the anchor path, at most 1,024 UTF-8
            bytes; rows whose anchor path starts with it, unanchored rows left out (ADR-0057 d4).
        origin_kinds (list[OriginKind] | Unset): Rows whose origin kind is one of these, combined
            with every other filter by AND; rows without a kind are left out (ADR-0063 d3).
        since (datetime.datetime | Unset):
        until (datetime.datetime | Unset):
        limit (int | Unset):  Default: 50.
        cursor (str | Unset):
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | MemoryPage
    """

    return sync_detailed(
        client=client,
        workarea=workarea,
        claim_key=claim_key,
        state=state,
        kind=kind,
        own=own,
        with_own=with_own,
        author=author,
        anchor_repo=anchor_repo,
        anchor_path_prefix=anchor_path_prefix,
        origin_kinds=origin_kinds,
        since=since,
        until=until,
        limit=limit,
        cursor=cursor,
        x_request_id=x_request_id,
        x_correlation_id=x_correlation_id,
        traceparent=traceparent,
    ).parsed


async def asyncio_detailed(
    *,
    client: AuthenticatedClient | Client,
    workarea: str | Unset = UNSET,
    claim_key: str | Unset = UNSET,
    state: ListMemoriesState | Unset = UNSET,
    kind: ListMemoriesKind | Unset = UNSET,
    own: bool | Unset = False,
    with_own: bool | Unset = UNSET,
    author: str | Unset = UNSET,
    anchor_repo: str | Unset = UNSET,
    anchor_path_prefix: str | Unset = UNSET,
    origin_kinds: list[OriginKind] | Unset = UNSET,
    since: datetime.datetime | Unset = UNSET,
    until: datetime.datetime | Unset = UNSET,
    limit: int | Unset = 50,
    cursor: str | Unset = UNSET,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> Response[ErrorEnvelope | MemoryPage]:
    """Structured, paginated listing of memory summaries.

     Requires: reader effective access; own=true admits any tenant role except operator
    (access.verb_ceiling) for its own rows only — every row the calling key authored, in every state,
    across all workareas minus its globally denied subtrees, no grant needed (ADR-0025 d13). With
    own=true and no workarea it lists every workarea the key wrote in. Without own, the listing returns
    promoted served tips within reader effective access in the workarea's subtree, defaulting to the
    key's default_workarea (ADR-0039 d4) — a self_signed version only when the scope node is its own
    workarea; a retired claim's tip, its retirement version, among them, flagged retirement true;
    others' held rows are never returned (R3). workarea is the subtree read (see the parameter);
    v0.3.0's workarea_id parameter and X-OML-Workarea header are request.validation naming workarea in
    details.renamed_to (ADR-0039 d7). Ordered created_at descending, then artifact_id descending; the
    cursor is bound to the calling key and expires under api.cursor_ttl.

    Args:
        workarea (str | Unset): A workarea, by its node id, its name or its qualified name.
            Resolved server-side before any check that reads state; an unknown name answers exactly
            what an unknown id answers. A user key resolves a bare name in its own user root's
            namespace, plus universe and its root's name; every other key in the shared namespace
            (ADR-0044 d5). A derived path, a value of three or more labels or one whose first label is
            universe, is neither, and is refused (request.validation).
        claim_key (str | Unset):
        state (ListMemoriesState | Unset):
        kind (ListMemoriesKind | Unset):
        own (bool | Unset):  Default: False.
        with_own (bool | Unset):
        author (str | Unset):
        anchor_repo (str | Unset): A repository, scheme, credentials and .git dropped
            (github.com/e8kor/open-memory-lab); no segment is . or .. (ADR-0057 d3). As a filter, rows
            whose anchor names it; unanchored rows are left out (d4).
        anchor_path_prefix (str | Unset): A byte prefix of the anchor path, at most 1,024 UTF-8
            bytes; rows whose anchor path starts with it, unanchored rows left out (ADR-0057 d4).
        origin_kinds (list[OriginKind] | Unset): Rows whose origin kind is one of these, combined
            with every other filter by AND; rows without a kind are left out (ADR-0063 d3).
        since (datetime.datetime | Unset):
        until (datetime.datetime | Unset):
        limit (int | Unset):  Default: 50.
        cursor (str | Unset):
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | MemoryPage]
    """

    kwargs = _get_kwargs(
        workarea=workarea,
        claim_key=claim_key,
        state=state,
        kind=kind,
        own=own,
        with_own=with_own,
        author=author,
        anchor_repo=anchor_repo,
        anchor_path_prefix=anchor_path_prefix,
        origin_kinds=origin_kinds,
        since=since,
        until=until,
        limit=limit,
        cursor=cursor,
        x_request_id=x_request_id,
        x_correlation_id=x_correlation_id,
        traceparent=traceparent,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    *,
    client: AuthenticatedClient | Client,
    workarea: str | Unset = UNSET,
    claim_key: str | Unset = UNSET,
    state: ListMemoriesState | Unset = UNSET,
    kind: ListMemoriesKind | Unset = UNSET,
    own: bool | Unset = False,
    with_own: bool | Unset = UNSET,
    author: str | Unset = UNSET,
    anchor_repo: str | Unset = UNSET,
    anchor_path_prefix: str | Unset = UNSET,
    origin_kinds: list[OriginKind] | Unset = UNSET,
    since: datetime.datetime | Unset = UNSET,
    until: datetime.datetime | Unset = UNSET,
    limit: int | Unset = 50,
    cursor: str | Unset = UNSET,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> ErrorEnvelope | MemoryPage | None:
    """Structured, paginated listing of memory summaries.

     Requires: reader effective access; own=true admits any tenant role except operator
    (access.verb_ceiling) for its own rows only — every row the calling key authored, in every state,
    across all workareas minus its globally denied subtrees, no grant needed (ADR-0025 d13). With
    own=true and no workarea it lists every workarea the key wrote in. Without own, the listing returns
    promoted served tips within reader effective access in the workarea's subtree, defaulting to the
    key's default_workarea (ADR-0039 d4) — a self_signed version only when the scope node is its own
    workarea; a retired claim's tip, its retirement version, among them, flagged retirement true;
    others' held rows are never returned (R3). workarea is the subtree read (see the parameter);
    v0.3.0's workarea_id parameter and X-OML-Workarea header are request.validation naming workarea in
    details.renamed_to (ADR-0039 d7). Ordered created_at descending, then artifact_id descending; the
    cursor is bound to the calling key and expires under api.cursor_ttl.

    Args:
        workarea (str | Unset): A workarea, by its node id, its name or its qualified name.
            Resolved server-side before any check that reads state; an unknown name answers exactly
            what an unknown id answers. A user key resolves a bare name in its own user root's
            namespace, plus universe and its root's name; every other key in the shared namespace
            (ADR-0044 d5). A derived path, a value of three or more labels or one whose first label is
            universe, is neither, and is refused (request.validation).
        claim_key (str | Unset):
        state (ListMemoriesState | Unset):
        kind (ListMemoriesKind | Unset):
        own (bool | Unset):  Default: False.
        with_own (bool | Unset):
        author (str | Unset):
        anchor_repo (str | Unset): A repository, scheme, credentials and .git dropped
            (github.com/e8kor/open-memory-lab); no segment is . or .. (ADR-0057 d3). As a filter, rows
            whose anchor names it; unanchored rows are left out (d4).
        anchor_path_prefix (str | Unset): A byte prefix of the anchor path, at most 1,024 UTF-8
            bytes; rows whose anchor path starts with it, unanchored rows left out (ADR-0057 d4).
        origin_kinds (list[OriginKind] | Unset): Rows whose origin kind is one of these, combined
            with every other filter by AND; rows without a kind are left out (ADR-0063 d3).
        since (datetime.datetime | Unset):
        until (datetime.datetime | Unset):
        limit (int | Unset):  Default: 50.
        cursor (str | Unset):
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | MemoryPage
    """

    return (
        await asyncio_detailed(
            client=client,
            workarea=workarea,
            claim_key=claim_key,
            state=state,
            kind=kind,
            own=own,
            with_own=with_own,
            author=author,
            anchor_repo=anchor_repo,
            anchor_path_prefix=anchor_path_prefix,
            origin_kinds=origin_kinds,
            since=since,
            until=until,
            limit=limit,
            cursor=cursor,
            x_request_id=x_request_id,
            x_correlation_id=x_correlation_id,
            traceparent=traceparent,
        )
    ).parsed
