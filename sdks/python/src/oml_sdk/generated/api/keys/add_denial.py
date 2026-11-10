from http import HTTPStatus
from typing import Any
from urllib.parse import quote

import httpx

from ...client import AuthenticatedClient, Client
from ...models.denial import Denial
from ...models.denial_request import DenialRequest
from ...models.error_envelope import ErrorEnvelope
from ...types import UNSET, Response, Unset


def _get_kwargs(
    key: str,
    *,
    body: DenialRequest,
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
        "url": "/v2/keys/{key}/denials".format(
            key=quote(str(key), safe=""),
        ),
    }

    _kwargs["json"] = body.to_dict()

    headers["Content-Type"] = "application/json"

    _kwargs["headers"] = headers
    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Denial | ErrorEnvelope:
    if response.status_code == 201:
        response_201 = Denial.from_dict(response.json())

        return response_201

    if response.status_code == 415:
        response_415 = ErrorEnvelope.from_dict(response.json())

        return response_415

    response_default = ErrorEnvelope.from_dict(response.json())

    return response_default


def _build_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Response[Denial | ErrorEnvelope]:
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
    body: DenialRequest,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
    idempotency_key: str,
) -> Response[Denial | ErrorEnvelope]:
    """Create or extend the key's global deny object — deny-wins across all its grants.

     A person's session key cuts a key off on universe alone, workarea universe, the key by name:
    <user>-web one of their own keys, <user>-admin any key of the tenant, never a session key; another
    capacity is access.verb_ceiling with details.reason session_capacity (ADR-0070 d5; ADR-0045 d9,
    d13). Otherwise requires: admin key role. Distinct from a per-grant deny mask. Journalled as
    key.denied; the denial's epoch is that event's journal seq. workarea takes an id or a name, and
    workarea_node_id, the v0.3.0 spelling, is refused with request.validation, details.renamed_to
    /workarea (ADR-0039 d7). A denial on an admin key is refused with request.validation, details.field
    key: denials attenuate grants, and an admin key carries none (docs/architecture/roles-and-keys.md,
    permission matrix footnote 6; ADR-0026 d15).

    Args:
        key (str): A key, by its id or its name, resolved server-side like every reference; a
            revoked or expired key's name still resolves (ADR-0026 d4, d10).
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        idempotency_key (str):
        body (DenialRequest):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Denial | ErrorEnvelope]
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
    body: DenialRequest,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
    idempotency_key: str,
) -> Denial | ErrorEnvelope | None:
    """Create or extend the key's global deny object — deny-wins across all its grants.

     A person's session key cuts a key off on universe alone, workarea universe, the key by name:
    <user>-web one of their own keys, <user>-admin any key of the tenant, never a session key; another
    capacity is access.verb_ceiling with details.reason session_capacity (ADR-0070 d5; ADR-0045 d9,
    d13). Otherwise requires: admin key role. Distinct from a per-grant deny mask. Journalled as
    key.denied; the denial's epoch is that event's journal seq. workarea takes an id or a name, and
    workarea_node_id, the v0.3.0 spelling, is refused with request.validation, details.renamed_to
    /workarea (ADR-0039 d7). A denial on an admin key is refused with request.validation, details.field
    key: denials attenuate grants, and an admin key carries none (docs/architecture/roles-and-keys.md,
    permission matrix footnote 6; ADR-0026 d15).

    Args:
        key (str): A key, by its id or its name, resolved server-side like every reference; a
            revoked or expired key's name still resolves (ADR-0026 d4, d10).
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        idempotency_key (str):
        body (DenialRequest):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Denial | ErrorEnvelope
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
    body: DenialRequest,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
    idempotency_key: str,
) -> Response[Denial | ErrorEnvelope]:
    """Create or extend the key's global deny object — deny-wins across all its grants.

     A person's session key cuts a key off on universe alone, workarea universe, the key by name:
    <user>-web one of their own keys, <user>-admin any key of the tenant, never a session key; another
    capacity is access.verb_ceiling with details.reason session_capacity (ADR-0070 d5; ADR-0045 d9,
    d13). Otherwise requires: admin key role. Distinct from a per-grant deny mask. Journalled as
    key.denied; the denial's epoch is that event's journal seq. workarea takes an id or a name, and
    workarea_node_id, the v0.3.0 spelling, is refused with request.validation, details.renamed_to
    /workarea (ADR-0039 d7). A denial on an admin key is refused with request.validation, details.field
    key: denials attenuate grants, and an admin key carries none (docs/architecture/roles-and-keys.md,
    permission matrix footnote 6; ADR-0026 d15).

    Args:
        key (str): A key, by its id or its name, resolved server-side like every reference; a
            revoked or expired key's name still resolves (ADR-0026 d4, d10).
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        idempotency_key (str):
        body (DenialRequest):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Denial | ErrorEnvelope]
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
    body: DenialRequest,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
    idempotency_key: str,
) -> Denial | ErrorEnvelope | None:
    """Create or extend the key's global deny object — deny-wins across all its grants.

     A person's session key cuts a key off on universe alone, workarea universe, the key by name:
    <user>-web one of their own keys, <user>-admin any key of the tenant, never a session key; another
    capacity is access.verb_ceiling with details.reason session_capacity (ADR-0070 d5; ADR-0045 d9,
    d13). Otherwise requires: admin key role. Distinct from a per-grant deny mask. Journalled as
    key.denied; the denial's epoch is that event's journal seq. workarea takes an id or a name, and
    workarea_node_id, the v0.3.0 spelling, is refused with request.validation, details.renamed_to
    /workarea (ADR-0039 d7). A denial on an admin key is refused with request.validation, details.field
    key: denials attenuate grants, and an admin key carries none (docs/architecture/roles-and-keys.md,
    permission matrix footnote 6; ADR-0026 d15).

    Args:
        key (str): A key, by its id or its name, resolved server-side like every reference; a
            revoked or expired key's name still resolves (ADR-0026 d4, d10).
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        idempotency_key (str):
        body (DenialRequest):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Denial | ErrorEnvelope
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
