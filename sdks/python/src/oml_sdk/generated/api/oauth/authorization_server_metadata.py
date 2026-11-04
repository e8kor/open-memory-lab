from http import HTTPStatus
from typing import Any, cast

import httpx

from ... import errors
from ...client import AuthenticatedClient, Client
from ...models.authorization_server_metadata import AuthorizationServerMetadata
from ...types import Response


def _get_kwargs() -> dict[str, Any]:

    _kwargs: dict[str, Any] = {
        "method": "get",
        "url": "/.well-known/oauth-authorization-server",
    }

    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Any | AuthorizationServerMetadata | None:
    if response.status_code == 200:
        response_200 = AuthorizationServerMetadata.from_dict(response.json())

        return response_200

    if response.status_code == 404:
        response_404 = cast(Any, None)
        return response_404

    if client.raise_on_unexpected_status:
        raise errors.UnexpectedStatus(response.status_code, response.content)
    else:
        return None


def _build_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Response[Any | AuthorizationServerMetadata]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    *,
    client: AuthenticatedClient | Client,
) -> Response[Any | AuthorizationServerMetadata]:
    """RFC 8414 authorization server metadata; 404 where oauth.issuer is unset.

     Unauthenticated, no tenant data, built from oauth.issuer alone. No OpenID Connect discovery document
    is served: OML issues no ID token (ADR-0031 d3).

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Any | AuthorizationServerMetadata]
    """

    kwargs = _get_kwargs()

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    *,
    client: AuthenticatedClient | Client,
) -> Any | AuthorizationServerMetadata | None:
    """RFC 8414 authorization server metadata; 404 where oauth.issuer is unset.

     Unauthenticated, no tenant data, built from oauth.issuer alone. No OpenID Connect discovery document
    is served: OML issues no ID token (ADR-0031 d3).

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Any | AuthorizationServerMetadata
    """

    return sync_detailed(
        client=client,
    ).parsed


async def asyncio_detailed(
    *,
    client: AuthenticatedClient | Client,
) -> Response[Any | AuthorizationServerMetadata]:
    """RFC 8414 authorization server metadata; 404 where oauth.issuer is unset.

     Unauthenticated, no tenant data, built from oauth.issuer alone. No OpenID Connect discovery document
    is served: OML issues no ID token (ADR-0031 d3).

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Any | AuthorizationServerMetadata]
    """

    kwargs = _get_kwargs()

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    *,
    client: AuthenticatedClient | Client,
) -> Any | AuthorizationServerMetadata | None:
    """RFC 8414 authorization server metadata; 404 where oauth.issuer is unset.

     Unauthenticated, no tenant data, built from oauth.issuer alone. No OpenID Connect discovery document
    is served: OML issues no ID token (ADR-0031 d3).

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Any | AuthorizationServerMetadata
    """

    return (
        await asyncio_detailed(
            client=client,
        )
    ).parsed
