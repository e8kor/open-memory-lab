from http import HTTPStatus
from typing import Any

import httpx

from ...client import AuthenticatedClient, Client
from ...models.error_envelope import ErrorEnvelope
from ...models.whoami import Whoami
from ...types import Response


def _get_kwargs() -> dict[str, Any]:

    _kwargs: dict[str, Any] = {
        "method": "get",
        "url": "/v2/whoami",
    }

    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> ErrorEnvelope | Whoami:
    if response.status_code == 200:
        response_200 = Whoami.from_dict(response.json())

        return response_200

    if response.status_code == 401:
        response_401 = ErrorEnvelope.from_dict(response.json())

        return response_401

    response_default = ErrorEnvelope.from_dict(response.json())

    return response_default


def _build_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Response[ErrorEnvelope | Whoami]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    *,
    client: AuthenticatedClient | Client,
) -> Response[ErrorEnvelope | Whoami]:
    """The calling key's own record — role, name, tenant, default workarea, crew and grants. Never its
    denials.

     Requires: any live key; it reads only its own record. Each reference is {id, name} and the default
    workarea carries its derived path. The key's denials, which name subtrees it may not read, are
    omitted; the read journals nothing, since it reads no claim. A revoked or unknown key answers
    auth.invalid as on every route (docs/api/http-api.md, Whoami; ADR-0039 d15).

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | Whoami]
    """

    kwargs = _get_kwargs()

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    *,
    client: AuthenticatedClient | Client,
) -> ErrorEnvelope | Whoami | None:
    """The calling key's own record — role, name, tenant, default workarea, crew and grants. Never its
    denials.

     Requires: any live key; it reads only its own record. Each reference is {id, name} and the default
    workarea carries its derived path. The key's denials, which name subtrees it may not read, are
    omitted; the read journals nothing, since it reads no claim. A revoked or unknown key answers
    auth.invalid as on every route (docs/api/http-api.md, Whoami; ADR-0039 d15).

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | Whoami
    """

    return sync_detailed(
        client=client,
    ).parsed


async def asyncio_detailed(
    *,
    client: AuthenticatedClient | Client,
) -> Response[ErrorEnvelope | Whoami]:
    """The calling key's own record — role, name, tenant, default workarea, crew and grants. Never its
    denials.

     Requires: any live key; it reads only its own record. Each reference is {id, name} and the default
    workarea carries its derived path. The key's denials, which name subtrees it may not read, are
    omitted; the read journals nothing, since it reads no claim. A revoked or unknown key answers
    auth.invalid as on every route (docs/api/http-api.md, Whoami; ADR-0039 d15).

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | Whoami]
    """

    kwargs = _get_kwargs()

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    *,
    client: AuthenticatedClient | Client,
) -> ErrorEnvelope | Whoami | None:
    """The calling key's own record — role, name, tenant, default workarea, crew and grants. Never its
    denials.

     Requires: any live key; it reads only its own record. Each reference is {id, name} and the default
    workarea carries its derived path. The key's denials, which name subtrees it may not read, are
    omitted; the read journals nothing, since it reads no claim. A revoked or unknown key answers
    auth.invalid as on every route (docs/api/http-api.md, Whoami; ADR-0039 d15).

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | Whoami
    """

    return (
        await asyncio_detailed(
            client=client,
        )
    ).parsed
