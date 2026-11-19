from http import HTTPStatus
from typing import Any
from urllib.parse import quote

import httpx

from ...client import AuthenticatedClient, Client
from ...models.error_envelope import ErrorEnvelope
from ...models.key_link import KeyLink
from ...types import UNSET, Response, Unset


def _get_kwargs(
    key: str,
    *,
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
        "url": "/v2/keys/{key}/unlink".format(
            key=quote(str(key), safe=""),
        ),
    }

    _kwargs["headers"] = headers
    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> ErrorEnvelope | KeyLink:
    if response.status_code == 200:
        response_200 = KeyLink.from_dict(response.json())

        return response_200

    if response.status_code == 403:
        response_403 = ErrorEnvelope.from_dict(response.json())

        return response_403

    if response.status_code == 404:
        response_404 = ErrorEnvelope.from_dict(response.json())

        return response_404

    response_default = ErrorEnvelope.from_dict(response.json())

    return response_default


def _build_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Response[ErrorEnvelope | KeyLink]:
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
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
    idempotency_key: str,
) -> Response[ErrorEnvelope | KeyLink]:
    """End a key's link to a person.

     Requires: a console admin session, <user>-admin through its capacity credential, in the key's own
    tenant; another role is access.verb_ceiling, a bearer admin key, which names no person, is
    request.validation with details.field key and details.reason session_admin_only, and no MCP tool or
    CLI command reaches the route (the operator, 2026-11-17: "Console only"). A key without a link is
    request.validation with details.field key and details.reason not_linked. Journalled as
    key.unlinked_person; the console's next act with the key answers auth.invalid with details.reason
    acting_key_not_linked, and the key no longer writes into the user's root. A revoked key is unlinked
    too. The Idempotency-Key is required and not replayed (ADR-0085 d16). Since v0.8.3.

    Args:
        key (str): A key, by its id or its name, resolved server-side like every reference; a
            revoked or expired key's name still resolves (ADR-0026 d4, d10).
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        idempotency_key (str):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | KeyLink]
    """

    kwargs = _get_kwargs(
        key=key,
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
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
    idempotency_key: str,
) -> ErrorEnvelope | KeyLink | None:
    """End a key's link to a person.

     Requires: a console admin session, <user>-admin through its capacity credential, in the key's own
    tenant; another role is access.verb_ceiling, a bearer admin key, which names no person, is
    request.validation with details.field key and details.reason session_admin_only, and no MCP tool or
    CLI command reaches the route (the operator, 2026-11-17: "Console only"). A key without a link is
    request.validation with details.field key and details.reason not_linked. Journalled as
    key.unlinked_person; the console's next act with the key answers auth.invalid with details.reason
    acting_key_not_linked, and the key no longer writes into the user's root. A revoked key is unlinked
    too. The Idempotency-Key is required and not replayed (ADR-0085 d16). Since v0.8.3.

    Args:
        key (str): A key, by its id or its name, resolved server-side like every reference; a
            revoked or expired key's name still resolves (ADR-0026 d4, d10).
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        idempotency_key (str):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | KeyLink
    """

    return sync_detailed(
        key=key,
        client=client,
        x_request_id=x_request_id,
        x_correlation_id=x_correlation_id,
        traceparent=traceparent,
        idempotency_key=idempotency_key,
    ).parsed


async def asyncio_detailed(
    key: str,
    *,
    client: AuthenticatedClient | Client,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
    idempotency_key: str,
) -> Response[ErrorEnvelope | KeyLink]:
    """End a key's link to a person.

     Requires: a console admin session, <user>-admin through its capacity credential, in the key's own
    tenant; another role is access.verb_ceiling, a bearer admin key, which names no person, is
    request.validation with details.field key and details.reason session_admin_only, and no MCP tool or
    CLI command reaches the route (the operator, 2026-11-17: "Console only"). A key without a link is
    request.validation with details.field key and details.reason not_linked. Journalled as
    key.unlinked_person; the console's next act with the key answers auth.invalid with details.reason
    acting_key_not_linked, and the key no longer writes into the user's root. A revoked key is unlinked
    too. The Idempotency-Key is required and not replayed (ADR-0085 d16). Since v0.8.3.

    Args:
        key (str): A key, by its id or its name, resolved server-side like every reference; a
            revoked or expired key's name still resolves (ADR-0026 d4, d10).
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        idempotency_key (str):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | KeyLink]
    """

    kwargs = _get_kwargs(
        key=key,
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
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
    idempotency_key: str,
) -> ErrorEnvelope | KeyLink | None:
    """End a key's link to a person.

     Requires: a console admin session, <user>-admin through its capacity credential, in the key's own
    tenant; another role is access.verb_ceiling, a bearer admin key, which names no person, is
    request.validation with details.field key and details.reason session_admin_only, and no MCP tool or
    CLI command reaches the route (the operator, 2026-11-17: "Console only"). A key without a link is
    request.validation with details.field key and details.reason not_linked. Journalled as
    key.unlinked_person; the console's next act with the key answers auth.invalid with details.reason
    acting_key_not_linked, and the key no longer writes into the user's root. A revoked key is unlinked
    too. The Idempotency-Key is required and not replayed (ADR-0085 d16). Since v0.8.3.

    Args:
        key (str): A key, by its id or its name, resolved server-side like every reference; a
            revoked or expired key's name still resolves (ADR-0026 d4, d10).
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        idempotency_key (str):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | KeyLink
    """

    return (
        await asyncio_detailed(
            key=key,
            client=client,
            x_request_id=x_request_id,
            x_correlation_id=x_correlation_id,
            traceparent=traceparent,
            idempotency_key=idempotency_key,
        )
    ).parsed
