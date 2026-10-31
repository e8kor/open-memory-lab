from http import HTTPStatus
from typing import Any

import httpx

from ...client import AuthenticatedClient, Client
from ...models.ban import Ban
from ...models.ban_preview import BanPreview
from ...models.ban_request import BanRequest
from ...models.error_envelope import ErrorEnvelope
from ...types import Response


def _get_kwargs(
    *,
    body: BanRequest,
    idempotency_key: str,
) -> dict[str, Any]:
    headers: dict[str, Any] = {}
    headers["Idempotency-Key"] = idempotency_key

    _kwargs: dict[str, Any] = {
        "method": "post",
        "url": "/v2/bans",
    }

    _kwargs["json"] = body.to_dict()

    headers["Content-Type"] = "application/json"

    _kwargs["headers"] = headers
    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Ban | BanPreview | ErrorEnvelope:
    if response.status_code == 200:
        response_200 = BanPreview.from_dict(response.json())

        return response_200

    if response.status_code == 201:
        response_201 = Ban.from_dict(response.json())

        return response_201

    if response.status_code == 415:
        response_415 = ErrorEnvelope.from_dict(response.json())

        return response_415

    response_default = ErrorEnvelope.from_dict(response.json())

    return response_default


def _build_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Response[Ban | BanPreview | ErrorEnvelope]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    *,
    client: AuthenticatedClient | Client,
    body: BanRequest,
    idempotency_key: str,
) -> Response[Ban | BanPreview | ErrorEnvelope]:
    """Ban an author or source, checked at every admission; dry_run returns the reach preview without
    journalling.

     Requires: admin key role.

    Args:
        idempotency_key (str):
        body (BanRequest):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Ban | BanPreview | ErrorEnvelope]
    """

    kwargs = _get_kwargs(
        body=body,
        idempotency_key=idempotency_key,
    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    *,
    client: AuthenticatedClient | Client,
    body: BanRequest,
    idempotency_key: str,
) -> Ban | BanPreview | ErrorEnvelope | None:
    """Ban an author or source, checked at every admission; dry_run returns the reach preview without
    journalling.

     Requires: admin key role.

    Args:
        idempotency_key (str):
        body (BanRequest):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Ban | BanPreview | ErrorEnvelope
    """

    return sync_detailed(
        client=client,
        body=body,
        idempotency_key=idempotency_key,
    ).parsed


async def asyncio_detailed(
    *,
    client: AuthenticatedClient | Client,
    body: BanRequest,
    idempotency_key: str,
) -> Response[Ban | BanPreview | ErrorEnvelope]:
    """Ban an author or source, checked at every admission; dry_run returns the reach preview without
    journalling.

     Requires: admin key role.

    Args:
        idempotency_key (str):
        body (BanRequest):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Ban | BanPreview | ErrorEnvelope]
    """

    kwargs = _get_kwargs(
        body=body,
        idempotency_key=idempotency_key,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    *,
    client: AuthenticatedClient | Client,
    body: BanRequest,
    idempotency_key: str,
) -> Ban | BanPreview | ErrorEnvelope | None:
    """Ban an author or source, checked at every admission; dry_run returns the reach preview without
    journalling.

     Requires: admin key role.

    Args:
        idempotency_key (str):
        body (BanRequest):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Ban | BanPreview | ErrorEnvelope
    """

    return (
        await asyncio_detailed(
            client=client,
            body=body,
            idempotency_key=idempotency_key,
        )
    ).parsed
