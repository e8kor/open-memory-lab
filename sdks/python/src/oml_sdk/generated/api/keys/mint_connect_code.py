from http import HTTPStatus
from typing import Any
from urllib.parse import quote

import httpx

from ...client import AuthenticatedClient, Client
from ...models.connect_code import ConnectCode
from ...models.error_envelope import ErrorEnvelope
from ...types import Response


def _get_kwargs(
    key: str,
    *,
    idempotency_key: str,
) -> dict[str, Any]:
    headers: dict[str, Any] = {}
    headers["Idempotency-Key"] = idempotency_key

    _kwargs: dict[str, Any] = {
        "method": "post",
        "url": "/v2/keys/{key}/connect-codes".format(
            key=quote(str(key), safe=""),
        ),
    }

    _kwargs["headers"] = headers
    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> ConnectCode | ErrorEnvelope:
    if response.status_code == 201:
        response_201 = ConnectCode.from_dict(response.json())

        return response_201

    if response.status_code == 404:
        response_404 = ErrorEnvelope.from_dict(response.json())

        return response_404

    if response.status_code == 422:
        response_422 = ErrorEnvelope.from_dict(response.json())

        return response_422

    response_default = ErrorEnvelope.from_dict(response.json())

    return response_default


def _build_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Response[ConnectCode | ErrorEnvelope]:
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
    idempotency_key: str,
) -> Response[ConnectCode | ErrorEnvelope]:
    """Mint a fresh one-time connect code for a connector key; voids the key's earlier codes.

     Requires: admin key role. The key must be a connector key (issued with connect_code: true, no bearer
    value), neither revoked nor expired, on a deployment with oauth.issuer set; otherwise 422
    oauth.connect_refused with details.reason (bearer_key, revoked, expired, not_configured). The code
    is shown exactly once, here; the server keeps only its SHA-256 digest, and connection.code_issued
    journals its id and expiry, never the code (ADR-0031 d7, d8). The same commit voids the key's
    earlier codes as docs/architecture/roles-and-keys.md (Connections) declares. As built the route
    keeps no idempotency record, so a retry mints another code and voids this one. Lifetime: the
    oauth.connect_code_ttl setting.

    Args:
        key (str): A key, by its id or its name, resolved server-side like every reference; a
            revoked or expired key's name still resolves (ADR-0026 d4, d10).
        idempotency_key (str):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ConnectCode | ErrorEnvelope]
    """

    kwargs = _get_kwargs(
        key=key,
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
    idempotency_key: str,
) -> ConnectCode | ErrorEnvelope | None:
    """Mint a fresh one-time connect code for a connector key; voids the key's earlier codes.

     Requires: admin key role. The key must be a connector key (issued with connect_code: true, no bearer
    value), neither revoked nor expired, on a deployment with oauth.issuer set; otherwise 422
    oauth.connect_refused with details.reason (bearer_key, revoked, expired, not_configured). The code
    is shown exactly once, here; the server keeps only its SHA-256 digest, and connection.code_issued
    journals its id and expiry, never the code (ADR-0031 d7, d8). The same commit voids the key's
    earlier codes as docs/architecture/roles-and-keys.md (Connections) declares. As built the route
    keeps no idempotency record, so a retry mints another code and voids this one. Lifetime: the
    oauth.connect_code_ttl setting.

    Args:
        key (str): A key, by its id or its name, resolved server-side like every reference; a
            revoked or expired key's name still resolves (ADR-0026 d4, d10).
        idempotency_key (str):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ConnectCode | ErrorEnvelope
    """

    return sync_detailed(
        key=key,
        client=client,
        idempotency_key=idempotency_key,
    ).parsed


async def asyncio_detailed(
    key: str,
    *,
    client: AuthenticatedClient | Client,
    idempotency_key: str,
) -> Response[ConnectCode | ErrorEnvelope]:
    """Mint a fresh one-time connect code for a connector key; voids the key's earlier codes.

     Requires: admin key role. The key must be a connector key (issued with connect_code: true, no bearer
    value), neither revoked nor expired, on a deployment with oauth.issuer set; otherwise 422
    oauth.connect_refused with details.reason (bearer_key, revoked, expired, not_configured). The code
    is shown exactly once, here; the server keeps only its SHA-256 digest, and connection.code_issued
    journals its id and expiry, never the code (ADR-0031 d7, d8). The same commit voids the key's
    earlier codes as docs/architecture/roles-and-keys.md (Connections) declares. As built the route
    keeps no idempotency record, so a retry mints another code and voids this one. Lifetime: the
    oauth.connect_code_ttl setting.

    Args:
        key (str): A key, by its id or its name, resolved server-side like every reference; a
            revoked or expired key's name still resolves (ADR-0026 d4, d10).
        idempotency_key (str):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ConnectCode | ErrorEnvelope]
    """

    kwargs = _get_kwargs(
        key=key,
        idempotency_key=idempotency_key,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    key: str,
    *,
    client: AuthenticatedClient | Client,
    idempotency_key: str,
) -> ConnectCode | ErrorEnvelope | None:
    """Mint a fresh one-time connect code for a connector key; voids the key's earlier codes.

     Requires: admin key role. The key must be a connector key (issued with connect_code: true, no bearer
    value), neither revoked nor expired, on a deployment with oauth.issuer set; otherwise 422
    oauth.connect_refused with details.reason (bearer_key, revoked, expired, not_configured). The code
    is shown exactly once, here; the server keeps only its SHA-256 digest, and connection.code_issued
    journals its id and expiry, never the code (ADR-0031 d7, d8). The same commit voids the key's
    earlier codes as docs/architecture/roles-and-keys.md (Connections) declares. As built the route
    keeps no idempotency record, so a retry mints another code and voids this one. Lifetime: the
    oauth.connect_code_ttl setting.

    Args:
        key (str): A key, by its id or its name, resolved server-side like every reference; a
            revoked or expired key's name still resolves (ADR-0026 d4, d10).
        idempotency_key (str):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ConnectCode | ErrorEnvelope
    """

    return (
        await asyncio_detailed(
            key=key,
            client=client,
            idempotency_key=idempotency_key,
        )
    ).parsed
