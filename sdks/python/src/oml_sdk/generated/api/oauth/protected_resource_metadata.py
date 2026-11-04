from http import HTTPStatus
from typing import Any, cast

import httpx

from ... import errors
from ...client import AuthenticatedClient, Client
from ...models.protected_resource_metadata import ProtectedResourceMetadata
from ...types import Response


def _get_kwargs() -> dict[str, Any]:

    _kwargs: dict[str, Any] = {
        "method": "get",
        "url": "/.well-known/oauth-protected-resource/mcp",
    }

    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Any | ProtectedResourceMetadata | None:
    if response.status_code == 200:
        response_200 = ProtectedResourceMetadata.from_dict(response.json())

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
) -> Response[Any | ProtectedResourceMetadata]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    *,
    client: AuthenticatedClient | Client,
) -> Response[Any | ProtectedResourceMetadata]:
    """RFC 9728 protected-resource metadata for the /mcp resource; 404 where oauth.issuer is unset.

     Unauthenticated, no tenant data. Built from oauth.issuer alone, never from the request Host.
    resource is exactly <issuer>/mcp (RFC 9728 §3.3); one authorization server, the issuer itself. The
    root form /.well-known/oauth-protected-resource is not served (ADR-0031 d3).

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Any | ProtectedResourceMetadata]
    """

    kwargs = _get_kwargs()

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    *,
    client: AuthenticatedClient | Client,
) -> Any | ProtectedResourceMetadata | None:
    """RFC 9728 protected-resource metadata for the /mcp resource; 404 where oauth.issuer is unset.

     Unauthenticated, no tenant data. Built from oauth.issuer alone, never from the request Host.
    resource is exactly <issuer>/mcp (RFC 9728 §3.3); one authorization server, the issuer itself. The
    root form /.well-known/oauth-protected-resource is not served (ADR-0031 d3).

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Any | ProtectedResourceMetadata
    """

    return sync_detailed(
        client=client,
    ).parsed


async def asyncio_detailed(
    *,
    client: AuthenticatedClient | Client,
) -> Response[Any | ProtectedResourceMetadata]:
    """RFC 9728 protected-resource metadata for the /mcp resource; 404 where oauth.issuer is unset.

     Unauthenticated, no tenant data. Built from oauth.issuer alone, never from the request Host.
    resource is exactly <issuer>/mcp (RFC 9728 §3.3); one authorization server, the issuer itself. The
    root form /.well-known/oauth-protected-resource is not served (ADR-0031 d3).

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Any | ProtectedResourceMetadata]
    """

    kwargs = _get_kwargs()

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    *,
    client: AuthenticatedClient | Client,
) -> Any | ProtectedResourceMetadata | None:
    """RFC 9728 protected-resource metadata for the /mcp resource; 404 where oauth.issuer is unset.

     Unauthenticated, no tenant data. Built from oauth.issuer alone, never from the request Host.
    resource is exactly <issuer>/mcp (RFC 9728 §3.3); one authorization server, the issuer itself. The
    root form /.well-known/oauth-protected-resource is not served (ADR-0031 d3).

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Any | ProtectedResourceMetadata
    """

    return (
        await asyncio_detailed(
            client=client,
        )
    ).parsed
