from http import HTTPStatus
from typing import Any
from urllib.parse import quote

import httpx

from ...client import AuthenticatedClient, Client
from ...models.error_envelope import ErrorEnvelope
from ...models.workarea_status import WorkareaStatus
from ...types import Response


def _get_kwargs(
    workarea_id: str,
) -> dict[str, Any]:

    _kwargs: dict[str, Any] = {
        "method": "get",
        "url": "/v2/workareas/{workarea_id}/status".format(
            workarea_id=quote(str(workarea_id), safe=""),
        ),
    }

    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> ErrorEnvelope | WorkareaStatus:
    if response.status_code == 200:
        response_200 = WorkareaStatus.from_dict(response.json())

        return response_200

    response_default = ErrorEnvelope.from_dict(response.json())

    return response_default


def _build_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Response[ErrorEnvelope | WorkareaStatus]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    workarea_id: str,
    *,
    client: AuthenticatedClient | Client,
) -> Response[ErrorEnvelope | WorkareaStatus]:
    """Status, the review policy object in force, pending transitions.

     Requires: reader effective access.

    Args:
        workarea_id (str):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | WorkareaStatus]
    """

    kwargs = _get_kwargs(
        workarea_id=workarea_id,
    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    workarea_id: str,
    *,
    client: AuthenticatedClient | Client,
) -> ErrorEnvelope | WorkareaStatus | None:
    """Status, the review policy object in force, pending transitions.

     Requires: reader effective access.

    Args:
        workarea_id (str):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | WorkareaStatus
    """

    return sync_detailed(
        workarea_id=workarea_id,
        client=client,
    ).parsed


async def asyncio_detailed(
    workarea_id: str,
    *,
    client: AuthenticatedClient | Client,
) -> Response[ErrorEnvelope | WorkareaStatus]:
    """Status, the review policy object in force, pending transitions.

     Requires: reader effective access.

    Args:
        workarea_id (str):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | WorkareaStatus]
    """

    kwargs = _get_kwargs(
        workarea_id=workarea_id,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    workarea_id: str,
    *,
    client: AuthenticatedClient | Client,
) -> ErrorEnvelope | WorkareaStatus | None:
    """Status, the review policy object in force, pending transitions.

     Requires: reader effective access.

    Args:
        workarea_id (str):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | WorkareaStatus
    """

    return (
        await asyncio_detailed(
            workarea_id=workarea_id,
            client=client,
        )
    ).parsed
