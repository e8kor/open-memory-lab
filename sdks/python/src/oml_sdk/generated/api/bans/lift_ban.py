from http import HTTPStatus
from typing import Any
from urllib.parse import quote

import httpx

from ...client import AuthenticatedClient, Client
from ...models.ban import Ban
from ...models.error_envelope import ErrorEnvelope
from ...types import Response


def _get_kwargs(
    ban_id: str,
    *,
    idempotency_key: str,
) -> dict[str, Any]:
    headers: dict[str, Any] = {}
    headers["Idempotency-Key"] = idempotency_key

    _kwargs: dict[str, Any] = {
        "method": "post",
        "url": "/v2/bans/{ban_id}/lift".format(
            ban_id=quote(str(ban_id), safe=""),
        ),
    }

    _kwargs["headers"] = headers
    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Ban | ErrorEnvelope:
    if response.status_code == 200:
        response_200 = Ban.from_dict(response.json())

        return response_200

    response_default = ErrorEnvelope.from_dict(response.json())

    return response_default


def _build_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Response[Ban | ErrorEnvelope]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    ban_id: str,
    *,
    client: AuthenticatedClient | Client,
    idempotency_key: str,
) -> Response[Ban | ErrorEnvelope]:
    """Journalled lift; the ban row stays as history (no silent removal of governance acts).

     Requires: admin key role.

    Args:
        ban_id (str):
        idempotency_key (str):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Ban | ErrorEnvelope]
    """

    kwargs = _get_kwargs(
        ban_id=ban_id,
        idempotency_key=idempotency_key,
    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    ban_id: str,
    *,
    client: AuthenticatedClient | Client,
    idempotency_key: str,
) -> Ban | ErrorEnvelope | None:
    """Journalled lift; the ban row stays as history (no silent removal of governance acts).

     Requires: admin key role.

    Args:
        ban_id (str):
        idempotency_key (str):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Ban | ErrorEnvelope
    """

    return sync_detailed(
        ban_id=ban_id,
        client=client,
        idempotency_key=idempotency_key,
    ).parsed


async def asyncio_detailed(
    ban_id: str,
    *,
    client: AuthenticatedClient | Client,
    idempotency_key: str,
) -> Response[Ban | ErrorEnvelope]:
    """Journalled lift; the ban row stays as history (no silent removal of governance acts).

     Requires: admin key role.

    Args:
        ban_id (str):
        idempotency_key (str):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Ban | ErrorEnvelope]
    """

    kwargs = _get_kwargs(
        ban_id=ban_id,
        idempotency_key=idempotency_key,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    ban_id: str,
    *,
    client: AuthenticatedClient | Client,
    idempotency_key: str,
) -> Ban | ErrorEnvelope | None:
    """Journalled lift; the ban row stays as history (no silent removal of governance acts).

     Requires: admin key role.

    Args:
        ban_id (str):
        idempotency_key (str):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Ban | ErrorEnvelope
    """

    return (
        await asyncio_detailed(
            ban_id=ban_id,
            client=client,
            idempotency_key=idempotency_key,
        )
    ).parsed
