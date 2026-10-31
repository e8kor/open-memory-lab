from http import HTTPStatus
from typing import Any
from urllib.parse import quote

import httpx

from ...client import AuthenticatedClient, Client
from ...models.error_envelope import ErrorEnvelope
from ...models.grant import Grant
from ...models.grant_request import GrantRequest
from ...types import Response


def _get_kwargs(
    key_id: str,
    *,
    body: GrantRequest,
    idempotency_key: str,
) -> dict[str, Any]:
    headers: dict[str, Any] = {}
    headers["Idempotency-Key"] = idempotency_key

    _kwargs: dict[str, Any] = {
        "method": "post",
        "url": "/v2/keys/{key_id}/grants".format(
            key_id=quote(str(key_id), safe=""),
        ),
    }

    _kwargs["json"] = body.to_dict()

    headers["Content-Type"] = "application/json"

    _kwargs["headers"] = headers
    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> ErrorEnvelope | Grant:
    if response.status_code == 201:
        response_201 = Grant.from_dict(response.json())

        return response_201

    if response.status_code == 415:
        response_415 = ErrorEnvelope.from_dict(response.json())

        return response_415

    response_default = ErrorEnvelope.from_dict(response.json())

    return response_default


def _build_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Response[ErrorEnvelope | Grant]:
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
    body: GrantRequest,
    idempotency_key: str,
) -> Response[ErrorEnvelope | Grant]:
    """Bind the key to a workarea node, by id or name (never a path), with optional per-grant deny masks;
    epoch bump.

     Requires: admin key role. A grant's role must be admissible under the key's role (the admissibility
    table in docs/architecture/roles-and-keys.md; access.issuer_ceiling with details {role, admissible}
    otherwise — one grant, so no grant_index); a dreamer-class grant on an admin key is refused first
    (access.role_exclusive — the two-key rule). The grant's epoch is its grant.issued journal seq.

    Args:
        key_id (str): A key, by its id or its name, resolved server-side like every reference; a
            revoked or expired key's name still resolves (ADR-0026 d4, d10).
        idempotency_key (str):
        body (GrantRequest):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | Grant]
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
    body: GrantRequest,
    idempotency_key: str,
) -> ErrorEnvelope | Grant | None:
    """Bind the key to a workarea node, by id or name (never a path), with optional per-grant deny masks;
    epoch bump.

     Requires: admin key role. A grant's role must be admissible under the key's role (the admissibility
    table in docs/architecture/roles-and-keys.md; access.issuer_ceiling with details {role, admissible}
    otherwise — one grant, so no grant_index); a dreamer-class grant on an admin key is refused first
    (access.role_exclusive — the two-key rule). The grant's epoch is its grant.issued journal seq.

    Args:
        key_id (str): A key, by its id or its name, resolved server-side like every reference; a
            revoked or expired key's name still resolves (ADR-0026 d4, d10).
        idempotency_key (str):
        body (GrantRequest):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | Grant
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
    body: GrantRequest,
    idempotency_key: str,
) -> Response[ErrorEnvelope | Grant]:
    """Bind the key to a workarea node, by id or name (never a path), with optional per-grant deny masks;
    epoch bump.

     Requires: admin key role. A grant's role must be admissible under the key's role (the admissibility
    table in docs/architecture/roles-and-keys.md; access.issuer_ceiling with details {role, admissible}
    otherwise — one grant, so no grant_index); a dreamer-class grant on an admin key is refused first
    (access.role_exclusive — the two-key rule). The grant's epoch is its grant.issued journal seq.

    Args:
        key_id (str): A key, by its id or its name, resolved server-side like every reference; a
            revoked or expired key's name still resolves (ADR-0026 d4, d10).
        idempotency_key (str):
        body (GrantRequest):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | Grant]
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
    body: GrantRequest,
    idempotency_key: str,
) -> ErrorEnvelope | Grant | None:
    """Bind the key to a workarea node, by id or name (never a path), with optional per-grant deny masks;
    epoch bump.

     Requires: admin key role. A grant's role must be admissible under the key's role (the admissibility
    table in docs/architecture/roles-and-keys.md; access.issuer_ceiling with details {role, admissible}
    otherwise — one grant, so no grant_index); a dreamer-class grant on an admin key is refused first
    (access.role_exclusive — the two-key rule). The grant's epoch is its grant.issued journal seq.

    Args:
        key_id (str): A key, by its id or its name, resolved server-side like every reference; a
            revoked or expired key's name still resolves (ADR-0026 d4, d10).
        idempotency_key (str):
        body (GrantRequest):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | Grant
    """

    return (
        await asyncio_detailed(
            key_id=key_id,
            client=client,
            body=body,
            idempotency_key=idempotency_key,
        )
    ).parsed
