from http import HTTPStatus
from typing import Any
from urllib.parse import quote

import httpx

from ...client import AuthenticatedClient, Client
from ...models.denial import Denial
from ...models.denial_request import DenialRequest
from ...models.error_envelope import ErrorEnvelope
from ...types import Response


def _get_kwargs(
    key_id: str,
    *,
    body: DenialRequest,
    idempotency_key: str,
) -> dict[str, Any]:
    headers: dict[str, Any] = {}
    headers["Idempotency-Key"] = idempotency_key

    _kwargs: dict[str, Any] = {
        "method": "post",
        "url": "/v2/keys/{key_id}/denials".format(
            key_id=quote(str(key_id), safe=""),
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
    key_id: str,
    *,
    client: AuthenticatedClient | Client,
    body: DenialRequest,
    idempotency_key: str,
) -> Response[Denial | ErrorEnvelope]:
    """Create or extend the key's global deny object — deny-wins across all its grants.

     Requires: admin key role. Distinct from a per-grant deny mask. Journalled as key.denied; the
    denial's epoch is that event's journal seq. A denial on an admin key is refused with
    request.validation, details.field key_id: denials attenuate grants, and an admin key carries none
    (docs/architecture/roles-and-keys.md, permission matrix footnote 6; ADR-0026 d15).

    Args:
        key_id (str): A key, by its id or its name, resolved server-side like every reference; a
            revoked or expired key's name still resolves (ADR-0026 d4, d10).
        idempotency_key (str):
        body (DenialRequest):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Denial | ErrorEnvelope]
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
    body: DenialRequest,
    idempotency_key: str,
) -> Denial | ErrorEnvelope | None:
    """Create or extend the key's global deny object — deny-wins across all its grants.

     Requires: admin key role. Distinct from a per-grant deny mask. Journalled as key.denied; the
    denial's epoch is that event's journal seq. A denial on an admin key is refused with
    request.validation, details.field key_id: denials attenuate grants, and an admin key carries none
    (docs/architecture/roles-and-keys.md, permission matrix footnote 6; ADR-0026 d15).

    Args:
        key_id (str): A key, by its id or its name, resolved server-side like every reference; a
            revoked or expired key's name still resolves (ADR-0026 d4, d10).
        idempotency_key (str):
        body (DenialRequest):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Denial | ErrorEnvelope
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
    body: DenialRequest,
    idempotency_key: str,
) -> Response[Denial | ErrorEnvelope]:
    """Create or extend the key's global deny object — deny-wins across all its grants.

     Requires: admin key role. Distinct from a per-grant deny mask. Journalled as key.denied; the
    denial's epoch is that event's journal seq. A denial on an admin key is refused with
    request.validation, details.field key_id: denials attenuate grants, and an admin key carries none
    (docs/architecture/roles-and-keys.md, permission matrix footnote 6; ADR-0026 d15).

    Args:
        key_id (str): A key, by its id or its name, resolved server-side like every reference; a
            revoked or expired key's name still resolves (ADR-0026 d4, d10).
        idempotency_key (str):
        body (DenialRequest):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Denial | ErrorEnvelope]
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
    body: DenialRequest,
    idempotency_key: str,
) -> Denial | ErrorEnvelope | None:
    """Create or extend the key's global deny object — deny-wins across all its grants.

     Requires: admin key role. Distinct from a per-grant deny mask. Journalled as key.denied; the
    denial's epoch is that event's journal seq. A denial on an admin key is refused with
    request.validation, details.field key_id: denials attenuate grants, and an admin key carries none
    (docs/architecture/roles-and-keys.md, permission matrix footnote 6; ADR-0026 d15).

    Args:
        key_id (str): A key, by its id or its name, resolved server-side like every reference; a
            revoked or expired key's name still resolves (ADR-0026 d4, d10).
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
            key_id=key_id,
            client=client,
            body=body,
            idempotency_key=idempotency_key,
        )
    ).parsed
