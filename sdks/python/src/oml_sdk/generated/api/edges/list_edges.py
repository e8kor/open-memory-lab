from http import HTTPStatus
from typing import Any

import httpx

from ...client import AuthenticatedClient, Client
from ...models.edge_page import EdgePage
from ...models.error_envelope import ErrorEnvelope
from ...models.list_edges_kind import ListEdgesKind
from ...types import UNSET, Response, Unset


def _get_kwargs(
    *,
    kind: ListEdgesKind,
    workarea_id: str | Unset = UNSET,
    artifact_id: str | Unset = UNSET,
    limit: int | Unset = 50,
    cursor: str | Unset = UNSET,
) -> dict[str, Any]:

    params: dict[str, Any] = {}

    json_kind = kind.value
    params["kind"] = json_kind

    json_workarea_id: str | Unset
    if isinstance(workarea_id, Unset):
        json_workarea_id = UNSET
    else:
        json_workarea_id = workarea_id
    params["workarea_id"] = json_workarea_id

    params["artifact_id"] = artifact_id

    params["limit"] = limit

    params["cursor"] = cursor

    params = {k: v for k, v in params.items() if v is not UNSET and v is not None}

    _kwargs: dict[str, Any] = {
        "method": "get",
        "url": "/v2/edges",
        "params": params,
    }

    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> EdgePage | ErrorEnvelope:
    if response.status_code == 200:
        response_200 = EdgePage.from_dict(response.json())

        return response_200

    response_default = ErrorEnvelope.from_dict(response.json())

    return response_default


def _build_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Response[EdgePage | ErrorEnvelope]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    *,
    client: AuthenticatedClient | Client,
    kind: ListEdgesKind,
    workarea_id: str | Unset = UNSET,
    artifact_id: str | Unset = UNSET,
    limit: int | Unset = 50,
    cursor: str | Unset = UNSET,
) -> Response[EdgePage | ErrorEnvelope]:
    """Paginated edge read of one kind, pending and ratified, scoped to read access.

     kind claim reads the claim graph and requires reader effective access; workarea_id is a subtree
    filter and artifact_id narrows to the edges touching one claim. kind brief reads the brief edges
    into the workarea workarea_id names, required then, at reader effective access on it, each served
    only when the key reads its claim (ADR-0046 d7). kind workarea, designed and not served, reads the
    labelled related edges touching the node workarea_id names, required then, and requires reader
    effective access on it, or the admin key role, which reads the structure of every workarea in its
    tenant without grants and never claim content (docs/architecture/roles-and-keys.md, permission
    matrix footnote 6; ADR-0026 d15); artifact_id is refused with request.validation. Edges confer zero
    access. One family for both kinds, each keeping its authority, refusal codes and journal events
    (ADR-0039 d9).

    Args:
        kind (ListEdgesKind):
        workarea_id (str | Unset): A workarea, by its node id, its name or its qualified name.
            Resolved server-side before any check that reads state; an unknown name answers exactly
            what an unknown id answers. A user key resolves a bare name in its own user root's
            namespace, plus universe and its root's name; every other key in the shared namespace
            (ADR-0044 d5). A derived path, a value of three or more labels or one whose first label is
            universe, is neither, and is refused (request.validation).
        artifact_id (str | Unset):
        limit (int | Unset):  Default: 50.
        cursor (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[EdgePage | ErrorEnvelope]
    """

    kwargs = _get_kwargs(
        kind=kind,
        workarea_id=workarea_id,
        artifact_id=artifact_id,
        limit=limit,
        cursor=cursor,
    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    *,
    client: AuthenticatedClient | Client,
    kind: ListEdgesKind,
    workarea_id: str | Unset = UNSET,
    artifact_id: str | Unset = UNSET,
    limit: int | Unset = 50,
    cursor: str | Unset = UNSET,
) -> EdgePage | ErrorEnvelope | None:
    """Paginated edge read of one kind, pending and ratified, scoped to read access.

     kind claim reads the claim graph and requires reader effective access; workarea_id is a subtree
    filter and artifact_id narrows to the edges touching one claim. kind brief reads the brief edges
    into the workarea workarea_id names, required then, at reader effective access on it, each served
    only when the key reads its claim (ADR-0046 d7). kind workarea, designed and not served, reads the
    labelled related edges touching the node workarea_id names, required then, and requires reader
    effective access on it, or the admin key role, which reads the structure of every workarea in its
    tenant without grants and never claim content (docs/architecture/roles-and-keys.md, permission
    matrix footnote 6; ADR-0026 d15); artifact_id is refused with request.validation. Edges confer zero
    access. One family for both kinds, each keeping its authority, refusal codes and journal events
    (ADR-0039 d9).

    Args:
        kind (ListEdgesKind):
        workarea_id (str | Unset): A workarea, by its node id, its name or its qualified name.
            Resolved server-side before any check that reads state; an unknown name answers exactly
            what an unknown id answers. A user key resolves a bare name in its own user root's
            namespace, plus universe and its root's name; every other key in the shared namespace
            (ADR-0044 d5). A derived path, a value of three or more labels or one whose first label is
            universe, is neither, and is refused (request.validation).
        artifact_id (str | Unset):
        limit (int | Unset):  Default: 50.
        cursor (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        EdgePage | ErrorEnvelope
    """

    return sync_detailed(
        client=client,
        kind=kind,
        workarea_id=workarea_id,
        artifact_id=artifact_id,
        limit=limit,
        cursor=cursor,
    ).parsed


async def asyncio_detailed(
    *,
    client: AuthenticatedClient | Client,
    kind: ListEdgesKind,
    workarea_id: str | Unset = UNSET,
    artifact_id: str | Unset = UNSET,
    limit: int | Unset = 50,
    cursor: str | Unset = UNSET,
) -> Response[EdgePage | ErrorEnvelope]:
    """Paginated edge read of one kind, pending and ratified, scoped to read access.

     kind claim reads the claim graph and requires reader effective access; workarea_id is a subtree
    filter and artifact_id narrows to the edges touching one claim. kind brief reads the brief edges
    into the workarea workarea_id names, required then, at reader effective access on it, each served
    only when the key reads its claim (ADR-0046 d7). kind workarea, designed and not served, reads the
    labelled related edges touching the node workarea_id names, required then, and requires reader
    effective access on it, or the admin key role, which reads the structure of every workarea in its
    tenant without grants and never claim content (docs/architecture/roles-and-keys.md, permission
    matrix footnote 6; ADR-0026 d15); artifact_id is refused with request.validation. Edges confer zero
    access. One family for both kinds, each keeping its authority, refusal codes and journal events
    (ADR-0039 d9).

    Args:
        kind (ListEdgesKind):
        workarea_id (str | Unset): A workarea, by its node id, its name or its qualified name.
            Resolved server-side before any check that reads state; an unknown name answers exactly
            what an unknown id answers. A user key resolves a bare name in its own user root's
            namespace, plus universe and its root's name; every other key in the shared namespace
            (ADR-0044 d5). A derived path, a value of three or more labels or one whose first label is
            universe, is neither, and is refused (request.validation).
        artifact_id (str | Unset):
        limit (int | Unset):  Default: 50.
        cursor (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[EdgePage | ErrorEnvelope]
    """

    kwargs = _get_kwargs(
        kind=kind,
        workarea_id=workarea_id,
        artifact_id=artifact_id,
        limit=limit,
        cursor=cursor,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    *,
    client: AuthenticatedClient | Client,
    kind: ListEdgesKind,
    workarea_id: str | Unset = UNSET,
    artifact_id: str | Unset = UNSET,
    limit: int | Unset = 50,
    cursor: str | Unset = UNSET,
) -> EdgePage | ErrorEnvelope | None:
    """Paginated edge read of one kind, pending and ratified, scoped to read access.

     kind claim reads the claim graph and requires reader effective access; workarea_id is a subtree
    filter and artifact_id narrows to the edges touching one claim. kind brief reads the brief edges
    into the workarea workarea_id names, required then, at reader effective access on it, each served
    only when the key reads its claim (ADR-0046 d7). kind workarea, designed and not served, reads the
    labelled related edges touching the node workarea_id names, required then, and requires reader
    effective access on it, or the admin key role, which reads the structure of every workarea in its
    tenant without grants and never claim content (docs/architecture/roles-and-keys.md, permission
    matrix footnote 6; ADR-0026 d15); artifact_id is refused with request.validation. Edges confer zero
    access. One family for both kinds, each keeping its authority, refusal codes and journal events
    (ADR-0039 d9).

    Args:
        kind (ListEdgesKind):
        workarea_id (str | Unset): A workarea, by its node id, its name or its qualified name.
            Resolved server-side before any check that reads state; an unknown name answers exactly
            what an unknown id answers. A user key resolves a bare name in its own user root's
            namespace, plus universe and its root's name; every other key in the shared namespace
            (ADR-0044 d5). A derived path, a value of three or more labels or one whose first label is
            universe, is neither, and is refused (request.validation).
        artifact_id (str | Unset):
        limit (int | Unset):  Default: 50.
        cursor (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        EdgePage | ErrorEnvelope
    """

    return (
        await asyncio_detailed(
            client=client,
            kind=kind,
            workarea_id=workarea_id,
            artifact_id=artifact_id,
            limit=limit,
            cursor=cursor,
        )
    ).parsed
