from http import HTTPStatus
from typing import Any
from urllib.parse import quote

import httpx

from ...client import AuthenticatedClient, Client
from ...models.error_envelope import ErrorEnvelope
from ...models.rotate_key_body import RotateKeyBody
from ...models.rotate_key_response_200 import RotateKeyResponse200
from ...types import UNSET, Response, Unset


def _get_kwargs(
    key: str,
    *,
    body: RotateKeyBody | Unset = UNSET,
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
        "url": "/v2/keys/{key}/rotate".format(
            key=quote(str(key), safe=""),
        ),
    }

    if not isinstance(body, Unset):
        _kwargs["json"] = body.to_dict()

    headers["Content-Type"] = "application/json"

    _kwargs["headers"] = headers
    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> ErrorEnvelope | RotateKeyResponse200:
    if response.status_code == 200:
        response_200 = RotateKeyResponse200.from_dict(response.json())

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
) -> Response[ErrorEnvelope | RotateKeyResponse200]:
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
    body: RotateKeyBody | Unset = UNSET,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
    idempotency_key: str,
) -> Response[ErrorEnvelope | RotateKeyResponse200]:
    """Give a bearer key a new value; the previous one stops at once or after a grace of at most 24 hours.

     Requires: an admin key role in the key's tenant, a person's <user>-admin session key included, for
    any key; a person's <user>-web session key for a key naming that person; or the key itself, so an
    agent rolls its own secret. Any other caller is access.verb_ceiling, and a person's session naming
    another person's key is access.not_found. The key keeps its id, name, grants, denials, limits,
    links, crew and history; only its value changes. grace_seconds, 0 to 86400, keeps the previous value
    authenticating until previous_valid_until; omitted or 0, it stops at once, and a later rotation ends
    any earlier grace. A session key, a connector key (mint a new connect code instead), a revoked key,
    a cut-off key and an expired key are request.validation with details.field key and details.reason
    session_key, connector_key, revoked, cut_off or expired; a longer grace is details.field
    /grace_seconds, details.reason grace_too_long. The value is answered once: the Idempotency-Key is
    claimed for the caller before the rotation, so a resubmission under it answers the recorded response
    without value, with Idempotency-Replay: true, and rotates nothing again. Journalled as key.rotated
    with the key, the rotating key and previous_valid_until, never a value or a hash. While a grace is
    open, the key's whoami serves rotation_grace_until and oml doctor prints key.rotation_grace_open
    (ADR-0096 d13, the operator's decision of 2026-11-25). Since v0.8.5.

    Args:
        key (str): A key, by its id or its name, resolved server-side like every reference; a
            revoked or expired key's name still resolves (ADR-0026 d4, d10).
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        idempotency_key (str):
        body (RotateKeyBody | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | RotateKeyResponse200]
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
    body: RotateKeyBody | Unset = UNSET,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
    idempotency_key: str,
) -> ErrorEnvelope | RotateKeyResponse200 | None:
    """Give a bearer key a new value; the previous one stops at once or after a grace of at most 24 hours.

     Requires: an admin key role in the key's tenant, a person's <user>-admin session key included, for
    any key; a person's <user>-web session key for a key naming that person; or the key itself, so an
    agent rolls its own secret. Any other caller is access.verb_ceiling, and a person's session naming
    another person's key is access.not_found. The key keeps its id, name, grants, denials, limits,
    links, crew and history; only its value changes. grace_seconds, 0 to 86400, keeps the previous value
    authenticating until previous_valid_until; omitted or 0, it stops at once, and a later rotation ends
    any earlier grace. A session key, a connector key (mint a new connect code instead), a revoked key,
    a cut-off key and an expired key are request.validation with details.field key and details.reason
    session_key, connector_key, revoked, cut_off or expired; a longer grace is details.field
    /grace_seconds, details.reason grace_too_long. The value is answered once: the Idempotency-Key is
    claimed for the caller before the rotation, so a resubmission under it answers the recorded response
    without value, with Idempotency-Replay: true, and rotates nothing again. Journalled as key.rotated
    with the key, the rotating key and previous_valid_until, never a value or a hash. While a grace is
    open, the key's whoami serves rotation_grace_until and oml doctor prints key.rotation_grace_open
    (ADR-0096 d13, the operator's decision of 2026-11-25). Since v0.8.5.

    Args:
        key (str): A key, by its id or its name, resolved server-side like every reference; a
            revoked or expired key's name still resolves (ADR-0026 d4, d10).
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        idempotency_key (str):
        body (RotateKeyBody | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | RotateKeyResponse200
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
    body: RotateKeyBody | Unset = UNSET,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
    idempotency_key: str,
) -> Response[ErrorEnvelope | RotateKeyResponse200]:
    """Give a bearer key a new value; the previous one stops at once or after a grace of at most 24 hours.

     Requires: an admin key role in the key's tenant, a person's <user>-admin session key included, for
    any key; a person's <user>-web session key for a key naming that person; or the key itself, so an
    agent rolls its own secret. Any other caller is access.verb_ceiling, and a person's session naming
    another person's key is access.not_found. The key keeps its id, name, grants, denials, limits,
    links, crew and history; only its value changes. grace_seconds, 0 to 86400, keeps the previous value
    authenticating until previous_valid_until; omitted or 0, it stops at once, and a later rotation ends
    any earlier grace. A session key, a connector key (mint a new connect code instead), a revoked key,
    a cut-off key and an expired key are request.validation with details.field key and details.reason
    session_key, connector_key, revoked, cut_off or expired; a longer grace is details.field
    /grace_seconds, details.reason grace_too_long. The value is answered once: the Idempotency-Key is
    claimed for the caller before the rotation, so a resubmission under it answers the recorded response
    without value, with Idempotency-Replay: true, and rotates nothing again. Journalled as key.rotated
    with the key, the rotating key and previous_valid_until, never a value or a hash. While a grace is
    open, the key's whoami serves rotation_grace_until and oml doctor prints key.rotation_grace_open
    (ADR-0096 d13, the operator's decision of 2026-11-25). Since v0.8.5.

    Args:
        key (str): A key, by its id or its name, resolved server-side like every reference; a
            revoked or expired key's name still resolves (ADR-0026 d4, d10).
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        idempotency_key (str):
        body (RotateKeyBody | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | RotateKeyResponse200]
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
    body: RotateKeyBody | Unset = UNSET,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
    idempotency_key: str,
) -> ErrorEnvelope | RotateKeyResponse200 | None:
    """Give a bearer key a new value; the previous one stops at once or after a grace of at most 24 hours.

     Requires: an admin key role in the key's tenant, a person's <user>-admin session key included, for
    any key; a person's <user>-web session key for a key naming that person; or the key itself, so an
    agent rolls its own secret. Any other caller is access.verb_ceiling, and a person's session naming
    another person's key is access.not_found. The key keeps its id, name, grants, denials, limits,
    links, crew and history; only its value changes. grace_seconds, 0 to 86400, keeps the previous value
    authenticating until previous_valid_until; omitted or 0, it stops at once, and a later rotation ends
    any earlier grace. A session key, a connector key (mint a new connect code instead), a revoked key,
    a cut-off key and an expired key are request.validation with details.field key and details.reason
    session_key, connector_key, revoked, cut_off or expired; a longer grace is details.field
    /grace_seconds, details.reason grace_too_long. The value is answered once: the Idempotency-Key is
    claimed for the caller before the rotation, so a resubmission under it answers the recorded response
    without value, with Idempotency-Replay: true, and rotates nothing again. Journalled as key.rotated
    with the key, the rotating key and previous_valid_until, never a value or a hash. While a grace is
    open, the key's whoami serves rotation_grace_until and oml doctor prints key.rotation_grace_open
    (ADR-0096 d13, the operator's decision of 2026-11-25). Since v0.8.5.

    Args:
        key (str): A key, by its id or its name, resolved server-side like every reference; a
            revoked or expired key's name still resolves (ADR-0026 d4, d10).
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        idempotency_key (str):
        body (RotateKeyBody | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | RotateKeyResponse200
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
