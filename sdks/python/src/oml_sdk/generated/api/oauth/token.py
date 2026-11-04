from http import HTTPStatus
from typing import Any

import httpx

from ... import errors
from ...client import AuthenticatedClient, Client
from ...models.o_auth_error import OAuthError
from ...models.token_request import TokenRequest
from ...models.token_response import TokenResponse
from ...types import Response


def _get_kwargs(
    *,
    body: TokenRequest,
) -> dict[str, Any]:
    headers: dict[str, Any] = {}

    _kwargs: dict[str, Any] = {
        "method": "post",
        "url": "/oauth/token",
    }

    _kwargs["data"] = body.to_dict()
    headers["Content-Type"] = "application/x-www-form-urlencoded"

    _kwargs["headers"] = headers
    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> OAuthError | TokenResponse | None:
    if response.status_code == 200:
        response_200 = TokenResponse.from_dict(response.json())

        return response_200

    if response.status_code == 400:
        response_400 = OAuthError.from_dict(response.json())

        return response_400

    if response.status_code == 401:
        response_401 = OAuthError.from_dict(response.json())

        return response_401

    if response.status_code == 415:
        response_415 = OAuthError.from_dict(response.json())

        return response_415

    if client.raise_on_unexpected_status:
        raise errors.UnexpectedStatus(response.status_code, response.content)
    else:
        return None


def _build_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Response[OAuthError | TokenResponse]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    *,
    client: AuthenticatedClient | Client,
    body: TokenRequest,
) -> Response[OAuthError | TokenResponse]:
    """Exchange an authorization code, or rotate a refresh token, for an access token and a new refresh
    token.

     Public clients only (token_endpoint_auth_method none). authorization_code requires the PKCE
    code_verifier and the same client_id, and opens the connection (connection.opened); redirect_uri is
    required exactly when the authorize request carried it and must then equal it, and when that request
    omitted it, redirect_uri may be omitted or must equal the client's one registered URI. refresh_token
    answers a new pair and marks the presented token rotated; which tokens a refresh accepts, and when
    reuse of a refresh token or an authorization code revokes the connection (connection.revoked), are
    declared on docs/architecture/roles-and-keys.md (Connections). The client is the one stored on the
    code's or refresh token's connection; the client tables are never read here, a client_id that
    differs from the connection's is invalid_grant, and invalid_client answers only a request that sends
    client authentication: 401 with WWW-Authenticate for an Authorization header, 400 for a
    client_secret in the body. A revoked or expired key, a voided code, or a resource that is not the
    current <issuer>/mcp answers invalid_grant. A code or refresh token issued to another client is
    invalid_grant too (RFC 6749 §5.2), and a parameter sent more than once is request.validation
    (invalid_request). Refusals are RFC 6749 §5.2 bodies whose error_description begins with the OML
    code (ADR-0031 d4, d10, d13). The token path makes no outbound call.

    Args:
        body (TokenRequest):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[OAuthError | TokenResponse]
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
    body: TokenRequest,
) -> OAuthError | TokenResponse | None:
    """Exchange an authorization code, or rotate a refresh token, for an access token and a new refresh
    token.

     Public clients only (token_endpoint_auth_method none). authorization_code requires the PKCE
    code_verifier and the same client_id, and opens the connection (connection.opened); redirect_uri is
    required exactly when the authorize request carried it and must then equal it, and when that request
    omitted it, redirect_uri may be omitted or must equal the client's one registered URI. refresh_token
    answers a new pair and marks the presented token rotated; which tokens a refresh accepts, and when
    reuse of a refresh token or an authorization code revokes the connection (connection.revoked), are
    declared on docs/architecture/roles-and-keys.md (Connections). The client is the one stored on the
    code's or refresh token's connection; the client tables are never read here, a client_id that
    differs from the connection's is invalid_grant, and invalid_client answers only a request that sends
    client authentication: 401 with WWW-Authenticate for an Authorization header, 400 for a
    client_secret in the body. A revoked or expired key, a voided code, or a resource that is not the
    current <issuer>/mcp answers invalid_grant. A code or refresh token issued to another client is
    invalid_grant too (RFC 6749 §5.2), and a parameter sent more than once is request.validation
    (invalid_request). Refusals are RFC 6749 §5.2 bodies whose error_description begins with the OML
    code (ADR-0031 d4, d10, d13). The token path makes no outbound call.

    Args:
        body (TokenRequest):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        OAuthError | TokenResponse
    """

    return sync_detailed(
        client=client,
        body=body,
    ).parsed


async def asyncio_detailed(
    *,
    client: AuthenticatedClient | Client,
    body: TokenRequest,
) -> Response[OAuthError | TokenResponse]:
    """Exchange an authorization code, or rotate a refresh token, for an access token and a new refresh
    token.

     Public clients only (token_endpoint_auth_method none). authorization_code requires the PKCE
    code_verifier and the same client_id, and opens the connection (connection.opened); redirect_uri is
    required exactly when the authorize request carried it and must then equal it, and when that request
    omitted it, redirect_uri may be omitted or must equal the client's one registered URI. refresh_token
    answers a new pair and marks the presented token rotated; which tokens a refresh accepts, and when
    reuse of a refresh token or an authorization code revokes the connection (connection.revoked), are
    declared on docs/architecture/roles-and-keys.md (Connections). The client is the one stored on the
    code's or refresh token's connection; the client tables are never read here, a client_id that
    differs from the connection's is invalid_grant, and invalid_client answers only a request that sends
    client authentication: 401 with WWW-Authenticate for an Authorization header, 400 for a
    client_secret in the body. A revoked or expired key, a voided code, or a resource that is not the
    current <issuer>/mcp answers invalid_grant. A code or refresh token issued to another client is
    invalid_grant too (RFC 6749 §5.2), and a parameter sent more than once is request.validation
    (invalid_request). Refusals are RFC 6749 §5.2 bodies whose error_description begins with the OML
    code (ADR-0031 d4, d10, d13). The token path makes no outbound call.

    Args:
        body (TokenRequest):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[OAuthError | TokenResponse]
    """

    kwargs = _get_kwargs(
        body=body,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    *,
    client: AuthenticatedClient | Client,
    body: TokenRequest,
) -> OAuthError | TokenResponse | None:
    """Exchange an authorization code, or rotate a refresh token, for an access token and a new refresh
    token.

     Public clients only (token_endpoint_auth_method none). authorization_code requires the PKCE
    code_verifier and the same client_id, and opens the connection (connection.opened); redirect_uri is
    required exactly when the authorize request carried it and must then equal it, and when that request
    omitted it, redirect_uri may be omitted or must equal the client's one registered URI. refresh_token
    answers a new pair and marks the presented token rotated; which tokens a refresh accepts, and when
    reuse of a refresh token or an authorization code revokes the connection (connection.revoked), are
    declared on docs/architecture/roles-and-keys.md (Connections). The client is the one stored on the
    code's or refresh token's connection; the client tables are never read here, a client_id that
    differs from the connection's is invalid_grant, and invalid_client answers only a request that sends
    client authentication: 401 with WWW-Authenticate for an Authorization header, 400 for a
    client_secret in the body. A revoked or expired key, a voided code, or a resource that is not the
    current <issuer>/mcp answers invalid_grant. A code or refresh token issued to another client is
    invalid_grant too (RFC 6749 §5.2), and a parameter sent more than once is request.validation
    (invalid_request). Refusals are RFC 6749 §5.2 bodies whose error_description begins with the OML
    code (ADR-0031 d4, d10, d13). The token path makes no outbound call.

    Args:
        body (TokenRequest):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        OAuthError | TokenResponse
    """

    return (
        await asyncio_detailed(
            client=client,
            body=body,
        )
    ).parsed
