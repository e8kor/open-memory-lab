import datetime
from http import HTTPStatus
from typing import Any

import httpx

from ...client import AuthenticatedClient, Client
from ...models.error_envelope import ErrorEnvelope
from ...models.memory_page import MemoryPage
from ...models.search_memories_kind import SearchMemoriesKind
from ...models.search_memories_state import SearchMemoriesState
from ...types import UNSET, Response, Unset


def _get_kwargs(
    *,
    workarea_id: str | Unset = UNSET,
    claim_key: str | Unset = UNSET,
    state: SearchMemoriesState | Unset = UNSET,
    kind: SearchMemoriesKind | Unset = UNSET,
    own: bool | Unset = False,
    since: datetime.datetime | Unset = UNSET,
    until: datetime.datetime | Unset = UNSET,
    limit: int | Unset = 50,
    cursor: str | Unset = UNSET,
    x_oml_workarea: str | Unset = UNSET,
) -> dict[str, Any]:
    headers: dict[str, Any] = {}
    if not isinstance(x_oml_workarea, Unset):
        headers["X-OML-Workarea"] = x_oml_workarea

    params: dict[str, Any] = {}

    json_workarea_id: str | Unset
    if isinstance(workarea_id, Unset):
        json_workarea_id = UNSET
    else:
        json_workarea_id = workarea_id
    params["workarea_id"] = json_workarea_id

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
    workarea_id: str | Unset = UNSET,
    claim_key: str | Unset = UNSET,
    state: SearchMemoriesState | Unset = UNSET,
    kind: SearchMemoriesKind | Unset = UNSET,
    own: bool | Unset = False,
    since: datetime.datetime | Unset = UNSET,
    until: datetime.datetime | Unset = UNSET,
    limit: int | Unset = 50,
    cursor: str | Unset = UNSET,
    x_oml_workarea: str | Unset = UNSET,
) -> Response[ErrorEnvelope | MemoryPage]:
    """Structured, paginated listing of memory summaries.

     Requires: reader effective access; own=true admits any tenant role except operator
    (access.verb_ceiling) for its own rows only — every row the calling key authored, in every state,
    across all workareas minus its globally denied subtrees, no grant needed (ADR-0025 d13). With
    own=true the listing resolves no workarea, and an X-OML-Workarea header is refused
    (request.validation). Without own, the listing returns promoted served tips within reader effective
    access, defaulting to the token workarea's subtree (ADR-0022 d4) — a self_signed version only when
    the scope node is its own workarea; a retired claim's tip, its retirement version, among them,
    flagged retirement true; others' held rows are never returned (R3). workarea_id is a subtree filter
    (see the parameter). Ordered created_at descending, then artifact_id descending; the cursor is bound
    to the calling key and expires under api.cursor_ttl.

    Args:
        workarea_id (str | Unset): A workarea, by its node id or its name. Resolved server-side
            before any check that reads state; an unknown name answers exactly what an unknown id
            answers. A derived path is neither, and is refused (request.validation).
        claim_key (str | Unset):
        state (SearchMemoriesState | Unset):
        kind (SearchMemoriesKind | Unset):
        own (bool | Unset):  Default: False.
        since (datetime.datetime | Unset):
        until (datetime.datetime | Unset):
        limit (int | Unset):  Default: 50.
        cursor (str | Unset):
        x_oml_workarea (str | Unset): A workarea, by its node id or its name. Resolved server-side
            before any check that reads state; an unknown name answers exactly what an unknown id
            answers. A derived path is neither, and is refused (request.validation).

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | MemoryPage]
    """

    kwargs = _get_kwargs(
        workarea_id=workarea_id,
        claim_key=claim_key,
        state=state,
        kind=kind,
        own=own,
        since=since,
        until=until,
        limit=limit,
        cursor=cursor,
        x_oml_workarea=x_oml_workarea,
    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    *,
    client: AuthenticatedClient | Client,
    workarea_id: str | Unset = UNSET,
    claim_key: str | Unset = UNSET,
    state: SearchMemoriesState | Unset = UNSET,
    kind: SearchMemoriesKind | Unset = UNSET,
    own: bool | Unset = False,
    since: datetime.datetime | Unset = UNSET,
    until: datetime.datetime | Unset = UNSET,
    limit: int | Unset = 50,
    cursor: str | Unset = UNSET,
    x_oml_workarea: str | Unset = UNSET,
) -> ErrorEnvelope | MemoryPage | None:
    """Structured, paginated listing of memory summaries.

     Requires: reader effective access; own=true admits any tenant role except operator
    (access.verb_ceiling) for its own rows only — every row the calling key authored, in every state,
    across all workareas minus its globally denied subtrees, no grant needed (ADR-0025 d13). With
    own=true the listing resolves no workarea, and an X-OML-Workarea header is refused
    (request.validation). Without own, the listing returns promoted served tips within reader effective
    access, defaulting to the token workarea's subtree (ADR-0022 d4) — a self_signed version only when
    the scope node is its own workarea; a retired claim's tip, its retirement version, among them,
    flagged retirement true; others' held rows are never returned (R3). workarea_id is a subtree filter
    (see the parameter). Ordered created_at descending, then artifact_id descending; the cursor is bound
    to the calling key and expires under api.cursor_ttl.

    Args:
        workarea_id (str | Unset): A workarea, by its node id or its name. Resolved server-side
            before any check that reads state; an unknown name answers exactly what an unknown id
            answers. A derived path is neither, and is refused (request.validation).
        claim_key (str | Unset):
        state (SearchMemoriesState | Unset):
        kind (SearchMemoriesKind | Unset):
        own (bool | Unset):  Default: False.
        since (datetime.datetime | Unset):
        until (datetime.datetime | Unset):
        limit (int | Unset):  Default: 50.
        cursor (str | Unset):
        x_oml_workarea (str | Unset): A workarea, by its node id or its name. Resolved server-side
            before any check that reads state; an unknown name answers exactly what an unknown id
            answers. A derived path is neither, and is refused (request.validation).

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | MemoryPage
    """

    return sync_detailed(
        client=client,
        workarea_id=workarea_id,
        claim_key=claim_key,
        state=state,
        kind=kind,
        own=own,
        since=since,
        until=until,
        limit=limit,
        cursor=cursor,
        x_oml_workarea=x_oml_workarea,
    ).parsed


async def asyncio_detailed(
    *,
    client: AuthenticatedClient | Client,
    workarea_id: str | Unset = UNSET,
    claim_key: str | Unset = UNSET,
    state: SearchMemoriesState | Unset = UNSET,
    kind: SearchMemoriesKind | Unset = UNSET,
    own: bool | Unset = False,
    since: datetime.datetime | Unset = UNSET,
    until: datetime.datetime | Unset = UNSET,
    limit: int | Unset = 50,
    cursor: str | Unset = UNSET,
    x_oml_workarea: str | Unset = UNSET,
) -> Response[ErrorEnvelope | MemoryPage]:
    """Structured, paginated listing of memory summaries.

     Requires: reader effective access; own=true admits any tenant role except operator
    (access.verb_ceiling) for its own rows only — every row the calling key authored, in every state,
    across all workareas minus its globally denied subtrees, no grant needed (ADR-0025 d13). With
    own=true the listing resolves no workarea, and an X-OML-Workarea header is refused
    (request.validation). Without own, the listing returns promoted served tips within reader effective
    access, defaulting to the token workarea's subtree (ADR-0022 d4) — a self_signed version only when
    the scope node is its own workarea; a retired claim's tip, its retirement version, among them,
    flagged retirement true; others' held rows are never returned (R3). workarea_id is a subtree filter
    (see the parameter). Ordered created_at descending, then artifact_id descending; the cursor is bound
    to the calling key and expires under api.cursor_ttl.

    Args:
        workarea_id (str | Unset): A workarea, by its node id or its name. Resolved server-side
            before any check that reads state; an unknown name answers exactly what an unknown id
            answers. A derived path is neither, and is refused (request.validation).
        claim_key (str | Unset):
        state (SearchMemoriesState | Unset):
        kind (SearchMemoriesKind | Unset):
        own (bool | Unset):  Default: False.
        since (datetime.datetime | Unset):
        until (datetime.datetime | Unset):
        limit (int | Unset):  Default: 50.
        cursor (str | Unset):
        x_oml_workarea (str | Unset): A workarea, by its node id or its name. Resolved server-side
            before any check that reads state; an unknown name answers exactly what an unknown id
            answers. A derived path is neither, and is refused (request.validation).

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | MemoryPage]
    """

    kwargs = _get_kwargs(
        workarea_id=workarea_id,
        claim_key=claim_key,
        state=state,
        kind=kind,
        own=own,
        since=since,
        until=until,
        limit=limit,
        cursor=cursor,
        x_oml_workarea=x_oml_workarea,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    *,
    client: AuthenticatedClient | Client,
    workarea_id: str | Unset = UNSET,
    claim_key: str | Unset = UNSET,
    state: SearchMemoriesState | Unset = UNSET,
    kind: SearchMemoriesKind | Unset = UNSET,
    own: bool | Unset = False,
    since: datetime.datetime | Unset = UNSET,
    until: datetime.datetime | Unset = UNSET,
    limit: int | Unset = 50,
    cursor: str | Unset = UNSET,
    x_oml_workarea: str | Unset = UNSET,
) -> ErrorEnvelope | MemoryPage | None:
    """Structured, paginated listing of memory summaries.

     Requires: reader effective access; own=true admits any tenant role except operator
    (access.verb_ceiling) for its own rows only — every row the calling key authored, in every state,
    across all workareas minus its globally denied subtrees, no grant needed (ADR-0025 d13). With
    own=true the listing resolves no workarea, and an X-OML-Workarea header is refused
    (request.validation). Without own, the listing returns promoted served tips within reader effective
    access, defaulting to the token workarea's subtree (ADR-0022 d4) — a self_signed version only when
    the scope node is its own workarea; a retired claim's tip, its retirement version, among them,
    flagged retirement true; others' held rows are never returned (R3). workarea_id is a subtree filter
    (see the parameter). Ordered created_at descending, then artifact_id descending; the cursor is bound
    to the calling key and expires under api.cursor_ttl.

    Args:
        workarea_id (str | Unset): A workarea, by its node id or its name. Resolved server-side
            before any check that reads state; an unknown name answers exactly what an unknown id
            answers. A derived path is neither, and is refused (request.validation).
        claim_key (str | Unset):
        state (SearchMemoriesState | Unset):
        kind (SearchMemoriesKind | Unset):
        own (bool | Unset):  Default: False.
        since (datetime.datetime | Unset):
        until (datetime.datetime | Unset):
        limit (int | Unset):  Default: 50.
        cursor (str | Unset):
        x_oml_workarea (str | Unset): A workarea, by its node id or its name. Resolved server-side
            before any check that reads state; an unknown name answers exactly what an unknown id
            answers. A derived path is neither, and is refused (request.validation).

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | MemoryPage
    """

    return (
        await asyncio_detailed(
            client=client,
            workarea_id=workarea_id,
            claim_key=claim_key,
            state=state,
            kind=kind,
            own=own,
            since=since,
            until=until,
            limit=limit,
            cursor=cursor,
            x_oml_workarea=x_oml_workarea,
        )
    ).parsed
