from http import HTTPStatus
from typing import Any

import httpx

from ...client import AuthenticatedClient, Client
from ...models.claim_edge_page import ClaimEdgePage
from ...models.error_envelope import ErrorEnvelope
from ...types import UNSET, Response, Unset


def _get_kwargs(
    *,
    workarea_id: str | Unset = UNSET,
    artifact_id: str | Unset = UNSET,
    limit: int | Unset = 50,
    cursor: str | Unset = UNSET,
) -> dict[str, Any]:

    params: dict[str, Any] = {}

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
        "url": "/v2/claim-edges",
        "params": params,
    }

    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> ClaimEdgePage | ErrorEnvelope:
    if response.status_code == 200:
        response_200 = ClaimEdgePage.from_dict(response.json())

        return response_200

    response_default = ErrorEnvelope.from_dict(response.json())

    return response_default


def _build_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Response[ClaimEdgePage | ErrorEnvelope]:
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
    artifact_id: str | Unset = UNSET,
    limit: int | Unset = 50,
    cursor: str | Unset = UNSET,
) -> Response[ClaimEdgePage | ErrorEnvelope]:
    """Paginated claim-graph read, pending and ratified, scoped to read access.

     Requires: reader effective access.

    Args:
        workarea_id (str | Unset): A workarea, by its node id or its name. Resolved server-side
            before any check that reads state; an unknown name answers exactly what an unknown id
            answers. A derived path is neither, and is refused (request.validation).
        artifact_id (str | Unset):
        limit (int | Unset):  Default: 50.
        cursor (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ClaimEdgePage | ErrorEnvelope]
    """

    kwargs = _get_kwargs(
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
    workarea_id: str | Unset = UNSET,
    artifact_id: str | Unset = UNSET,
    limit: int | Unset = 50,
    cursor: str | Unset = UNSET,
) -> ClaimEdgePage | ErrorEnvelope | None:
    """Paginated claim-graph read, pending and ratified, scoped to read access.

     Requires: reader effective access.

    Args:
        workarea_id (str | Unset): A workarea, by its node id or its name. Resolved server-side
            before any check that reads state; an unknown name answers exactly what an unknown id
            answers. A derived path is neither, and is refused (request.validation).
        artifact_id (str | Unset):
        limit (int | Unset):  Default: 50.
        cursor (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ClaimEdgePage | ErrorEnvelope
    """

    return sync_detailed(
        client=client,
        workarea_id=workarea_id,
        artifact_id=artifact_id,
        limit=limit,
        cursor=cursor,
    ).parsed


async def asyncio_detailed(
    *,
    client: AuthenticatedClient | Client,
    workarea_id: str | Unset = UNSET,
    artifact_id: str | Unset = UNSET,
    limit: int | Unset = 50,
    cursor: str | Unset = UNSET,
) -> Response[ClaimEdgePage | ErrorEnvelope]:
    """Paginated claim-graph read, pending and ratified, scoped to read access.

     Requires: reader effective access.

    Args:
        workarea_id (str | Unset): A workarea, by its node id or its name. Resolved server-side
            before any check that reads state; an unknown name answers exactly what an unknown id
            answers. A derived path is neither, and is refused (request.validation).
        artifact_id (str | Unset):
        limit (int | Unset):  Default: 50.
        cursor (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ClaimEdgePage | ErrorEnvelope]
    """

    kwargs = _get_kwargs(
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
    workarea_id: str | Unset = UNSET,
    artifact_id: str | Unset = UNSET,
    limit: int | Unset = 50,
    cursor: str | Unset = UNSET,
) -> ClaimEdgePage | ErrorEnvelope | None:
    """Paginated claim-graph read, pending and ratified, scoped to read access.

     Requires: reader effective access.

    Args:
        workarea_id (str | Unset): A workarea, by its node id or its name. Resolved server-side
            before any check that reads state; an unknown name answers exactly what an unknown id
            answers. A derived path is neither, and is refused (request.validation).
        artifact_id (str | Unset):
        limit (int | Unset):  Default: 50.
        cursor (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ClaimEdgePage | ErrorEnvelope
    """

    return (
        await asyncio_detailed(
            client=client,
            workarea_id=workarea_id,
            artifact_id=artifact_id,
            limit=limit,
            cursor=cursor,
        )
    ).parsed
