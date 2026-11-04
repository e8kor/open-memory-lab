from http import HTTPStatus
from typing import Any
from urllib.parse import quote
from uuid import UUID

import httpx

from ...client import AuthenticatedClient, Client
from ...models.connection import Connection
from ...models.error_envelope import ErrorEnvelope
from ...models.revoke_connection_body import RevokeConnectionBody
from ...types import UNSET, Response, Unset


def _get_kwargs(
    key: str,
    connection_id: UUID,
    *,
    body: RevokeConnectionBody | Unset = UNSET,
    idempotency_key: str,
) -> dict[str, Any]:
    headers: dict[str, Any] = {}
    headers["Idempotency-Key"] = idempotency_key

    _kwargs: dict[str, Any] = {
        "method": "post",
        "url": "/v2/keys/{key}/connections/{connection_id}/revoke".format(
            key=quote(str(key), safe=""),
            connection_id=quote(str(connection_id), safe=""),
        ),
    }

    if not isinstance(body, Unset):
        _kwargs["json"] = body.to_dict()

    headers["Content-Type"] = "application/json"

    _kwargs["headers"] = headers
    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Connection | ErrorEnvelope:
    if response.status_code == 200:
        response_200 = Connection.from_dict(response.json())

        return response_200

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
) -> Response[Connection | ErrorEnvelope]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    key: str,
    connection_id: UUID,
    *,
    client: AuthenticatedClient | Client,
    body: RevokeConnectionBody | Unset = UNSET,
    idempotency_key: str,
) -> Response[Connection | ErrorEnvelope]:
    """Journalled revocation of one connection.

     Requires: admin key role. Journals connection.revoked with cause admin, which ends the connection at
    its next request and voids the key's codes as docs/architecture/roles-and-keys.md (Connections)
    declares (ADR-0031 d8, d11). An authorized connection is not journalled or listed, and naming one
    answers 404 access.not_found, as an unknown id does. Served whether or not oauth.issuer is set.

    Args:
        key (str): A key, by its id or its name, resolved server-side like every reference; a
            revoked or expired key's name still resolves (ADR-0026 d4, d10).
        connection_id (UUID):
        idempotency_key (str):
        body (RevokeConnectionBody | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Connection | ErrorEnvelope]
    """

    kwargs = _get_kwargs(
        key=key,
        connection_id=connection_id,
        body=body,
        idempotency_key=idempotency_key,
    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    key: str,
    connection_id: UUID,
    *,
    client: AuthenticatedClient | Client,
    body: RevokeConnectionBody | Unset = UNSET,
    idempotency_key: str,
) -> Connection | ErrorEnvelope | None:
    """Journalled revocation of one connection.

     Requires: admin key role. Journals connection.revoked with cause admin, which ends the connection at
    its next request and voids the key's codes as docs/architecture/roles-and-keys.md (Connections)
    declares (ADR-0031 d8, d11). An authorized connection is not journalled or listed, and naming one
    answers 404 access.not_found, as an unknown id does. Served whether or not oauth.issuer is set.

    Args:
        key (str): A key, by its id or its name, resolved server-side like every reference; a
            revoked or expired key's name still resolves (ADR-0026 d4, d10).
        connection_id (UUID):
        idempotency_key (str):
        body (RevokeConnectionBody | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Connection | ErrorEnvelope
    """

    return sync_detailed(
        key=key,
        connection_id=connection_id,
        client=client,
        body=body,
        idempotency_key=idempotency_key,
    ).parsed


async def asyncio_detailed(
    key: str,
    connection_id: UUID,
    *,
    client: AuthenticatedClient | Client,
    body: RevokeConnectionBody | Unset = UNSET,
    idempotency_key: str,
) -> Response[Connection | ErrorEnvelope]:
    """Journalled revocation of one connection.

     Requires: admin key role. Journals connection.revoked with cause admin, which ends the connection at
    its next request and voids the key's codes as docs/architecture/roles-and-keys.md (Connections)
    declares (ADR-0031 d8, d11). An authorized connection is not journalled or listed, and naming one
    answers 404 access.not_found, as an unknown id does. Served whether or not oauth.issuer is set.

    Args:
        key (str): A key, by its id or its name, resolved server-side like every reference; a
            revoked or expired key's name still resolves (ADR-0026 d4, d10).
        connection_id (UUID):
        idempotency_key (str):
        body (RevokeConnectionBody | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Connection | ErrorEnvelope]
    """

    kwargs = _get_kwargs(
        key=key,
        connection_id=connection_id,
        body=body,
        idempotency_key=idempotency_key,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    key: str,
    connection_id: UUID,
    *,
    client: AuthenticatedClient | Client,
    body: RevokeConnectionBody | Unset = UNSET,
    idempotency_key: str,
) -> Connection | ErrorEnvelope | None:
    """Journalled revocation of one connection.

     Requires: admin key role. Journals connection.revoked with cause admin, which ends the connection at
    its next request and voids the key's codes as docs/architecture/roles-and-keys.md (Connections)
    declares (ADR-0031 d8, d11). An authorized connection is not journalled or listed, and naming one
    answers 404 access.not_found, as an unknown id does. Served whether or not oauth.issuer is set.

    Args:
        key (str): A key, by its id or its name, resolved server-side like every reference; a
            revoked or expired key's name still resolves (ADR-0026 d4, d10).
        connection_id (UUID):
        idempotency_key (str):
        body (RevokeConnectionBody | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Connection | ErrorEnvelope
    """

    return (
        await asyncio_detailed(
            key=key,
            connection_id=connection_id,
            client=client,
            body=body,
            idempotency_key=idempotency_key,
        )
    ).parsed
