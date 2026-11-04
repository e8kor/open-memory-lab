from http import HTTPStatus
from typing import Any, cast

import httpx

from ... import errors
from ...client import AuthenticatedClient, Client
from ...models.web_sign_in_body import WebSignInBody
from ...types import Response


def _get_kwargs(
    *,
    body: WebSignInBody,
) -> dict[str, Any]:
    headers: dict[str, Any] = {}

    _kwargs: dict[str, Any] = {
        "method": "post",
        "url": "/web/sign-in",
    }

    _kwargs["data"] = body.to_dict()
    headers["Content-Type"] = "application/x-www-form-urlencoded"

    _kwargs["headers"] = headers
    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Any | str | None:
    if response.status_code == 303:
        response_303 = cast(Any, None)
        return response_303

    if response.status_code == 403:
        response_403 = response.text
        return response_403

    if client.raise_on_unexpected_status:
        raise errors.UnexpectedStatus(response.status_code, response.content)
    else:
        return None


def _build_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Response[Any | str]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    *,
    client: AuthenticatedClient | Client,
    body: WebSignInBody,
) -> Response[Any | str]:
    """Start a provider's sign-in — the code flow with S256 PKCE, state and, for Google, a nonce.

     The same-origin check of POST /oauth/authorize, then a 303 to the provider with the sealed __Host-
    oml_signin cookie set (SameSite=Lax, ten minutes). Writes no row (ADR-0045 d3). Since v0.5.0.

    Args:
        body (WebSignInBody):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Any | str]
    """

    kwargs = _get_kwargs(
        body=body,
    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    *,
    client: AuthenticatedClient | Client,
    body: WebSignInBody,
) -> Any | str | None:
    """Start a provider's sign-in — the code flow with S256 PKCE, state and, for Google, a nonce.

     The same-origin check of POST /oauth/authorize, then a 303 to the provider with the sealed __Host-
    oml_signin cookie set (SameSite=Lax, ten minutes). Writes no row (ADR-0045 d3). Since v0.5.0.

    Args:
        body (WebSignInBody):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Any | str
    """

    return sync_detailed(
        client=client,
        body=body,
    ).parsed


async def asyncio_detailed(
    *,
    client: AuthenticatedClient | Client,
    body: WebSignInBody,
) -> Response[Any | str]:
    """Start a provider's sign-in — the code flow with S256 PKCE, state and, for Google, a nonce.

     The same-origin check of POST /oauth/authorize, then a 303 to the provider with the sealed __Host-
    oml_signin cookie set (SameSite=Lax, ten minutes). Writes no row (ADR-0045 d3). Since v0.5.0.

    Args:
        body (WebSignInBody):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Any | str]
    """

    kwargs = _get_kwargs(
        body=body,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    *,
    client: AuthenticatedClient | Client,
    body: WebSignInBody,
) -> Any | str | None:
    """Start a provider's sign-in — the code flow with S256 PKCE, state and, for Google, a nonce.

     The same-origin check of POST /oauth/authorize, then a 303 to the provider with the sealed __Host-
    oml_signin cookie set (SameSite=Lax, ten minutes). Writes no row (ADR-0045 d3). Since v0.5.0.

    Args:
        body (WebSignInBody):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Any | str
    """

    return (
        await asyncio_detailed(
            client=client,
            body=body,
        )
    ).parsed
