from http import HTTPStatus
from typing import Any
from urllib.parse import quote

import httpx

from ...client import AuthenticatedClient, Client
from ...models.error_envelope import ErrorEnvelope
from ...models.key_issued import KeyIssued
from ...models.key_rotate_request import KeyRotateRequest
from ...types import Response


def _get_kwargs(
    key_id: str,
    *,
    body: KeyRotateRequest,
    idempotency_key: str,
) -> dict[str, Any]:
    headers: dict[str, Any] = {}
    headers["Idempotency-Key"] = idempotency_key

    _kwargs: dict[str, Any] = {
        "method": "post",
        "url": "/v2/keys/{key_id}/rotate".format(
            key_id=quote(str(key_id), safe=""),
        ),
    }

    _kwargs["json"] = body.to_dict()

    headers["Content-Type"] = "application/json"

    _kwargs["headers"] = headers
    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> ErrorEnvelope | KeyIssued:
    if response.status_code == 201:
        response_201 = KeyIssued.from_dict(response.json())

        return response_201

    if response.status_code == 415:
        response_415 = ErrorEnvelope.from_dict(response.json())

        return response_415

    response_default = ErrorEnvelope.from_dict(response.json())

    return response_default


def _build_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Response[ErrorEnvelope | KeyIssued]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    key_id: str,
    *,
    client: AuthenticatedClient | Client,
    body: KeyRotateRequest,
    idempotency_key: str,
) -> Response[ErrorEnvelope | KeyIssued]:
    """Mint a successor key (new key_id, linked by rotated_from, grants reproduced from the journalled
    records); the old key enters a bounded grace window, then is revoked.

     Requires: admin key role. One journalled act (key.rotated) carries both key ids, the successor's
    name and the grace deadline; at the deadline the old key is revoked (journalled, epoch bump). The
    successor takes its own name (409 name.taken when held), since both keys are live through the grace
    window, and carries the old key's crew (ADR-0026 d9). Per key in v1; bulk crew rotation is open —
    FR-6.

    Args:
        key_id (str): A key, by its id or its name, resolved server-side like every reference; a
            revoked or expired key's name still resolves (ADR-0026 d4, d10).
        idempotency_key (str):
        body (KeyRotateRequest):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | KeyIssued]
    """

    kwargs = _get_kwargs(
        key_id=key_id,
        body=body,
        idempotency_key=idempotency_key,
    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    key_id: str,
    *,
    client: AuthenticatedClient | Client,
    body: KeyRotateRequest,
    idempotency_key: str,
) -> ErrorEnvelope | KeyIssued | None:
    """Mint a successor key (new key_id, linked by rotated_from, grants reproduced from the journalled
    records); the old key enters a bounded grace window, then is revoked.

     Requires: admin key role. One journalled act (key.rotated) carries both key ids, the successor's
    name and the grace deadline; at the deadline the old key is revoked (journalled, epoch bump). The
    successor takes its own name (409 name.taken when held), since both keys are live through the grace
    window, and carries the old key's crew (ADR-0026 d9). Per key in v1; bulk crew rotation is open —
    FR-6.

    Args:
        key_id (str): A key, by its id or its name, resolved server-side like every reference; a
            revoked or expired key's name still resolves (ADR-0026 d4, d10).
        idempotency_key (str):
        body (KeyRotateRequest):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | KeyIssued
    """

    return sync_detailed(
        key_id=key_id,
        client=client,
        body=body,
        idempotency_key=idempotency_key,
    ).parsed


async def asyncio_detailed(
    key_id: str,
    *,
    client: AuthenticatedClient | Client,
    body: KeyRotateRequest,
    idempotency_key: str,
) -> Response[ErrorEnvelope | KeyIssued]:
    """Mint a successor key (new key_id, linked by rotated_from, grants reproduced from the journalled
    records); the old key enters a bounded grace window, then is revoked.

     Requires: admin key role. One journalled act (key.rotated) carries both key ids, the successor's
    name and the grace deadline; at the deadline the old key is revoked (journalled, epoch bump). The
    successor takes its own name (409 name.taken when held), since both keys are live through the grace
    window, and carries the old key's crew (ADR-0026 d9). Per key in v1; bulk crew rotation is open —
    FR-6.

    Args:
        key_id (str): A key, by its id or its name, resolved server-side like every reference; a
            revoked or expired key's name still resolves (ADR-0026 d4, d10).
        idempotency_key (str):
        body (KeyRotateRequest):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | KeyIssued]
    """

    kwargs = _get_kwargs(
        key_id=key_id,
        body=body,
        idempotency_key=idempotency_key,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    key_id: str,
    *,
    client: AuthenticatedClient | Client,
    body: KeyRotateRequest,
    idempotency_key: str,
) -> ErrorEnvelope | KeyIssued | None:
    """Mint a successor key (new key_id, linked by rotated_from, grants reproduced from the journalled
    records); the old key enters a bounded grace window, then is revoked.

     Requires: admin key role. One journalled act (key.rotated) carries both key ids, the successor's
    name and the grace deadline; at the deadline the old key is revoked (journalled, epoch bump). The
    successor takes its own name (409 name.taken when held), since both keys are live through the grace
    window, and carries the old key's crew (ADR-0026 d9). Per key in v1; bulk crew rotation is open —
    FR-6.

    Args:
        key_id (str): A key, by its id or its name, resolved server-side like every reference; a
            revoked or expired key's name still resolves (ADR-0026 d4, d10).
        idempotency_key (str):
        body (KeyRotateRequest):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | KeyIssued
    """

    return (
        await asyncio_detailed(
            key_id=key_id,
            client=client,
            body=body,
            idempotency_key=idempotency_key,
        )
    ).parsed
