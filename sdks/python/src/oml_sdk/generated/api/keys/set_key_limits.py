from http import HTTPStatus
from typing import Any
from urllib.parse import quote

import httpx

from ...client import AuthenticatedClient, Client
from ...models.error_envelope import ErrorEnvelope
from ...models.key_limits import KeyLimits
from ...models.key_limits_request import KeyLimitsRequest
from ...types import UNSET, Response, Unset


def _get_kwargs(
    key: str,
    *,
    body: KeyLimitsRequest,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
    idempotency_key: str,
) -> dict[str, Any]:
    headers: dict[str, Any] = {}
    if not isinstance(x_request_id, Unset):
        headers["X-Request-Id"] = x_request_id

    if not isinstance(x_correlation_id, Unset):
        headers["X-Correlation-Id"] = x_correlation_id

    if not isinstance(traceparent, Unset):
        headers["traceparent"] = traceparent

    headers["Idempotency-Key"] = idempotency_key

    _kwargs: dict[str, Any] = {
        "method": "post",
        "url": "/v2/keys/{key}/limits".format(
            key=quote(str(key), safe=""),
        ),
    }

    _kwargs["json"] = body.to_dict()

    headers["Content-Type"] = "application/json"

    _kwargs["headers"] = headers
    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> ErrorEnvelope | KeyLimits:
    if response.status_code == 200:
        response_200 = KeyLimits.from_dict(response.json())

        return response_200

    if response.status_code == 403:
        response_403 = ErrorEnvelope.from_dict(response.json())

        return response_403

    if response.status_code == 404:
        response_404 = ErrorEnvelope.from_dict(response.json())

        return response_404

    if response.status_code == 415:
        response_415 = ErrorEnvelope.from_dict(response.json())

        return response_415

    response_default = ErrorEnvelope.from_dict(response.json())

    return response_default


def _build_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Response[ErrorEnvelope | KeyLimits]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    key: str,
    *,
    client: AuthenticatedClient | Client,
    body: KeyLimitsRequest,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
    idempotency_key: str,
) -> Response[ErrorEnvelope | KeyLimits]:
    """Set or clear a key's expiry and its requests per minute.

     Requires: admin key role, in the key's own tenant; any other role is access.verb_ceiling, and no MCP
    tool reaches the route. A member sent replaces the value, null clears it, and a member not sent
    keeps it; a body that names neither is request.validation. expiry is later than the server's clock
    (details.field /expiry) and requests_per_minute from 1 to 1000000 (details.field
    /requests_per_minute). The key is never the calling key, a session key, a revoked key or an admin
    key: request.validation with details.field key and details.reason own_key, session_key, revoked or
    admin_key. No key that can change limits carries one, so none moves its own date or raises its own
    rate, and no admin key ends another by a date. Journalled as key.limits_set with both values as they
    stand after the change; the Idempotency-Key is required and not replayed, so a request sent twice
    journals two events that leave the same limits. Moving an expired key's expiry later makes it
    authenticate again; the rate holds from the key's next request (ADR-0079 d5; since v0.7.3).

    Args:
        key (str): A key, by its id or its name, resolved server-side like every reference; a
            revoked or expired key's name still resolves (ADR-0026 d4, d10).
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        idempotency_key (str):
        body (KeyLimitsRequest): A member sent replaces the value, null clears it, and a member
            not sent keeps it (ADR-0079 d5).

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | KeyLimits]
    """

    kwargs = _get_kwargs(
        key=key,
        body=body,
        x_request_id=x_request_id,
        x_correlation_id=x_correlation_id,
        traceparent=traceparent,
        idempotency_key=idempotency_key,
    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    key: str,
    *,
    client: AuthenticatedClient | Client,
    body: KeyLimitsRequest,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
    idempotency_key: str,
) -> ErrorEnvelope | KeyLimits | None:
    """Set or clear a key's expiry and its requests per minute.

     Requires: admin key role, in the key's own tenant; any other role is access.verb_ceiling, and no MCP
    tool reaches the route. A member sent replaces the value, null clears it, and a member not sent
    keeps it; a body that names neither is request.validation. expiry is later than the server's clock
    (details.field /expiry) and requests_per_minute from 1 to 1000000 (details.field
    /requests_per_minute). The key is never the calling key, a session key, a revoked key or an admin
    key: request.validation with details.field key and details.reason own_key, session_key, revoked or
    admin_key. No key that can change limits carries one, so none moves its own date or raises its own
    rate, and no admin key ends another by a date. Journalled as key.limits_set with both values as they
    stand after the change; the Idempotency-Key is required and not replayed, so a request sent twice
    journals two events that leave the same limits. Moving an expired key's expiry later makes it
    authenticate again; the rate holds from the key's next request (ADR-0079 d5; since v0.7.3).

    Args:
        key (str): A key, by its id or its name, resolved server-side like every reference; a
            revoked or expired key's name still resolves (ADR-0026 d4, d10).
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        idempotency_key (str):
        body (KeyLimitsRequest): A member sent replaces the value, null clears it, and a member
            not sent keeps it (ADR-0079 d5).

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | KeyLimits
    """

    return sync_detailed(
        key=key,
        client=client,
        body=body,
        x_request_id=x_request_id,
        x_correlation_id=x_correlation_id,
        traceparent=traceparent,
        idempotency_key=idempotency_key,
    ).parsed


async def asyncio_detailed(
    key: str,
    *,
    client: AuthenticatedClient | Client,
    body: KeyLimitsRequest,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
    idempotency_key: str,
) -> Response[ErrorEnvelope | KeyLimits]:
    """Set or clear a key's expiry and its requests per minute.

     Requires: admin key role, in the key's own tenant; any other role is access.verb_ceiling, and no MCP
    tool reaches the route. A member sent replaces the value, null clears it, and a member not sent
    keeps it; a body that names neither is request.validation. expiry is later than the server's clock
    (details.field /expiry) and requests_per_minute from 1 to 1000000 (details.field
    /requests_per_minute). The key is never the calling key, a session key, a revoked key or an admin
    key: request.validation with details.field key and details.reason own_key, session_key, revoked or
    admin_key. No key that can change limits carries one, so none moves its own date or raises its own
    rate, and no admin key ends another by a date. Journalled as key.limits_set with both values as they
    stand after the change; the Idempotency-Key is required and not replayed, so a request sent twice
    journals two events that leave the same limits. Moving an expired key's expiry later makes it
    authenticate again; the rate holds from the key's next request (ADR-0079 d5; since v0.7.3).

    Args:
        key (str): A key, by its id or its name, resolved server-side like every reference; a
            revoked or expired key's name still resolves (ADR-0026 d4, d10).
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        idempotency_key (str):
        body (KeyLimitsRequest): A member sent replaces the value, null clears it, and a member
            not sent keeps it (ADR-0079 d5).

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | KeyLimits]
    """

    kwargs = _get_kwargs(
        key=key,
        body=body,
        x_request_id=x_request_id,
        x_correlation_id=x_correlation_id,
        traceparent=traceparent,
        idempotency_key=idempotency_key,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    key: str,
    *,
    client: AuthenticatedClient | Client,
    body: KeyLimitsRequest,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
    idempotency_key: str,
) -> ErrorEnvelope | KeyLimits | None:
    """Set or clear a key's expiry and its requests per minute.

     Requires: admin key role, in the key's own tenant; any other role is access.verb_ceiling, and no MCP
    tool reaches the route. A member sent replaces the value, null clears it, and a member not sent
    keeps it; a body that names neither is request.validation. expiry is later than the server's clock
    (details.field /expiry) and requests_per_minute from 1 to 1000000 (details.field
    /requests_per_minute). The key is never the calling key, a session key, a revoked key or an admin
    key: request.validation with details.field key and details.reason own_key, session_key, revoked or
    admin_key. No key that can change limits carries one, so none moves its own date or raises its own
    rate, and no admin key ends another by a date. Journalled as key.limits_set with both values as they
    stand after the change; the Idempotency-Key is required and not replayed, so a request sent twice
    journals two events that leave the same limits. Moving an expired key's expiry later makes it
    authenticate again; the rate holds from the key's next request (ADR-0079 d5; since v0.7.3).

    Args:
        key (str): A key, by its id or its name, resolved server-side like every reference; a
            revoked or expired key's name still resolves (ADR-0026 d4, d10).
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        idempotency_key (str):
        body (KeyLimitsRequest): A member sent replaces the value, null clears it, and a member
            not sent keeps it (ADR-0079 d5).

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | KeyLimits
    """

    return (
        await asyncio_detailed(
            key=key,
            client=client,
            body=body,
            x_request_id=x_request_id,
            x_correlation_id=x_correlation_id,
            traceparent=traceparent,
            idempotency_key=idempotency_key,
        )
    ).parsed
