from http import HTTPStatus
from typing import Any, cast

import httpx

from ... import errors
from ...client import AuthenticatedClient, Client
from ...models.authorize_code_challenge_method import AuthorizeCodeChallengeMethod
from ...models.authorize_response_type import AuthorizeResponseType
from ...models.authorize_scope import AuthorizeScope
from ...types import UNSET, Response, Unset


def _get_kwargs(
    *,
    response_type: AuthorizeResponseType,
    client_id: str,
    redirect_uri: str | Unset = UNSET,
    code_challenge: str | Unset = UNSET,
    code_challenge_method: AuthorizeCodeChallengeMethod | Unset = UNSET,
    state: str | Unset = UNSET,
    resource: str | Unset = UNSET,
    scope: AuthorizeScope | Unset = UNSET,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> dict[str, Any]:
    headers: dict[str, Any] = {}
    if not isinstance(x_request_id, Unset):
        headers["X-Request-Id"] = x_request_id

    if not isinstance(x_correlation_id, Unset):
        headers["X-Correlation-Id"] = x_correlation_id

    if not isinstance(traceparent, Unset):
        headers["traceparent"] = traceparent

    params: dict[str, Any] = {}

    json_response_type = response_type.value
    params["response_type"] = json_response_type

    params["client_id"] = client_id

    params["redirect_uri"] = redirect_uri

    params["code_challenge"] = code_challenge

    json_code_challenge_method: str | Unset = UNSET
    if not isinstance(code_challenge_method, Unset):
        json_code_challenge_method = code_challenge_method.value

    params["code_challenge_method"] = json_code_challenge_method

    params["state"] = state

    params["resource"] = resource

    json_scope: str | Unset = UNSET
    if not isinstance(scope, Unset):
        json_scope = scope.value

    params["scope"] = json_scope

    params = {k: v for k, v in params.items() if v is not UNSET and v is not None}

    _kwargs: dict[str, Any] = {
        "method": "get",
        "url": "/oauth/authorize",
        "params": params,
    }

    _kwargs["headers"] = headers
    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Any | str | None:
    if response.status_code == 200:
        response_200 = response.text
        return response_200

    if response.status_code == 302:
        response_302 = cast(Any, None)
        return response_302

    if response.status_code == 400:
        response_400 = response.text
        return response_400

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
    response_type: AuthorizeResponseType,
    client_id: str,
    redirect_uri: str | Unset = UNSET,
    code_challenge: str | Unset = UNSET,
    code_challenge_method: AuthorizeCodeChallengeMethod | Unset = UNSET,
    state: str | Unset = UNSET,
    resource: str | Unset = UNSET,
    scope: AuthorizeScope | Unset = UNSET,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> Response[Any | str]:
    """Start an authorization request — validates the client, redirect URI and S256 PKCE, then renders the
    connect-code page.

     Unauthenticated; the person proves authority by the connect code (ADR-0031 d9). The parameter
    bounds, no parameter sent more than once among them, are checked first, before any fetch. A
    parameter past its bound (a repeated one and a present code_challenge that does not match the
    pattern included), a refused client or a refused redirect URI is a page, never a redirect
    (request.validation, oauth.client_invalid, oauth.redirect_refused); any later refusal,
    oauth.pkce_required for an absent code_challenge or an absent or non-S256 code_challenge_method
    among them, redirects with the RFC error, state and iss (RFC 9207). The authorization request writes
    no row: the page carries it as a handle sealed by an AEAD under a key the lead holds only in memory,
    so opening pages is not capped, and a full document cache evicts rather than refuses (ADR-0031 d5,
    d9). The page sets no cookie and carries the headers ADR-0031 d9 fixes: a Content-Security-Policy
    with default-src none, form-action self plus the redirect origin and frame-ancestors none, Cache-
    Control no-store, Referrer-Policy same-origin (no-referrer would make the browser send Origin: null
    on the page's own POST), X-Content-Type-Options nosniff.

    Args:
        response_type (AuthorizeResponseType):
        client_id (str):
        redirect_uri (str | Unset):
        code_challenge (str | Unset):
        code_challenge_method (AuthorizeCodeChallengeMethod | Unset):
        state (str | Unset):
        resource (str | Unset):
        scope (AuthorizeScope | Unset):
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Any | str]
    """

    kwargs = _get_kwargs(
        response_type=response_type,
        client_id=client_id,
        redirect_uri=redirect_uri,
        code_challenge=code_challenge,
        code_challenge_method=code_challenge_method,
        state=state,
        resource=resource,
        scope=scope,
        x_request_id=x_request_id,
        x_correlation_id=x_correlation_id,
        traceparent=traceparent,
    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    *,
    client: AuthenticatedClient | Client,
    response_type: AuthorizeResponseType,
    client_id: str,
    redirect_uri: str | Unset = UNSET,
    code_challenge: str | Unset = UNSET,
    code_challenge_method: AuthorizeCodeChallengeMethod | Unset = UNSET,
    state: str | Unset = UNSET,
    resource: str | Unset = UNSET,
    scope: AuthorizeScope | Unset = UNSET,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> Any | str | None:
    """Start an authorization request — validates the client, redirect URI and S256 PKCE, then renders the
    connect-code page.

     Unauthenticated; the person proves authority by the connect code (ADR-0031 d9). The parameter
    bounds, no parameter sent more than once among them, are checked first, before any fetch. A
    parameter past its bound (a repeated one and a present code_challenge that does not match the
    pattern included), a refused client or a refused redirect URI is a page, never a redirect
    (request.validation, oauth.client_invalid, oauth.redirect_refused); any later refusal,
    oauth.pkce_required for an absent code_challenge or an absent or non-S256 code_challenge_method
    among them, redirects with the RFC error, state and iss (RFC 9207). The authorization request writes
    no row: the page carries it as a handle sealed by an AEAD under a key the lead holds only in memory,
    so opening pages is not capped, and a full document cache evicts rather than refuses (ADR-0031 d5,
    d9). The page sets no cookie and carries the headers ADR-0031 d9 fixes: a Content-Security-Policy
    with default-src none, form-action self plus the redirect origin and frame-ancestors none, Cache-
    Control no-store, Referrer-Policy same-origin (no-referrer would make the browser send Origin: null
    on the page's own POST), X-Content-Type-Options nosniff.

    Args:
        response_type (AuthorizeResponseType):
        client_id (str):
        redirect_uri (str | Unset):
        code_challenge (str | Unset):
        code_challenge_method (AuthorizeCodeChallengeMethod | Unset):
        state (str | Unset):
        resource (str | Unset):
        scope (AuthorizeScope | Unset):
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Any | str
    """

    return sync_detailed(
        client=client,
        response_type=response_type,
        client_id=client_id,
        redirect_uri=redirect_uri,
        code_challenge=code_challenge,
        code_challenge_method=code_challenge_method,
        state=state,
        resource=resource,
        scope=scope,
        x_request_id=x_request_id,
        x_correlation_id=x_correlation_id,
        traceparent=traceparent,
    ).parsed


async def asyncio_detailed(
    *,
    client: AuthenticatedClient | Client,
    response_type: AuthorizeResponseType,
    client_id: str,
    redirect_uri: str | Unset = UNSET,
    code_challenge: str | Unset = UNSET,
    code_challenge_method: AuthorizeCodeChallengeMethod | Unset = UNSET,
    state: str | Unset = UNSET,
    resource: str | Unset = UNSET,
    scope: AuthorizeScope | Unset = UNSET,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> Response[Any | str]:
    """Start an authorization request — validates the client, redirect URI and S256 PKCE, then renders the
    connect-code page.

     Unauthenticated; the person proves authority by the connect code (ADR-0031 d9). The parameter
    bounds, no parameter sent more than once among them, are checked first, before any fetch. A
    parameter past its bound (a repeated one and a present code_challenge that does not match the
    pattern included), a refused client or a refused redirect URI is a page, never a redirect
    (request.validation, oauth.client_invalid, oauth.redirect_refused); any later refusal,
    oauth.pkce_required for an absent code_challenge or an absent or non-S256 code_challenge_method
    among them, redirects with the RFC error, state and iss (RFC 9207). The authorization request writes
    no row: the page carries it as a handle sealed by an AEAD under a key the lead holds only in memory,
    so opening pages is not capped, and a full document cache evicts rather than refuses (ADR-0031 d5,
    d9). The page sets no cookie and carries the headers ADR-0031 d9 fixes: a Content-Security-Policy
    with default-src none, form-action self plus the redirect origin and frame-ancestors none, Cache-
    Control no-store, Referrer-Policy same-origin (no-referrer would make the browser send Origin: null
    on the page's own POST), X-Content-Type-Options nosniff.

    Args:
        response_type (AuthorizeResponseType):
        client_id (str):
        redirect_uri (str | Unset):
        code_challenge (str | Unset):
        code_challenge_method (AuthorizeCodeChallengeMethod | Unset):
        state (str | Unset):
        resource (str | Unset):
        scope (AuthorizeScope | Unset):
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Any | str]
    """

    kwargs = _get_kwargs(
        response_type=response_type,
        client_id=client_id,
        redirect_uri=redirect_uri,
        code_challenge=code_challenge,
        code_challenge_method=code_challenge_method,
        state=state,
        resource=resource,
        scope=scope,
        x_request_id=x_request_id,
        x_correlation_id=x_correlation_id,
        traceparent=traceparent,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    *,
    client: AuthenticatedClient | Client,
    response_type: AuthorizeResponseType,
    client_id: str,
    redirect_uri: str | Unset = UNSET,
    code_challenge: str | Unset = UNSET,
    code_challenge_method: AuthorizeCodeChallengeMethod | Unset = UNSET,
    state: str | Unset = UNSET,
    resource: str | Unset = UNSET,
    scope: AuthorizeScope | Unset = UNSET,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> Any | str | None:
    """Start an authorization request — validates the client, redirect URI and S256 PKCE, then renders the
    connect-code page.

     Unauthenticated; the person proves authority by the connect code (ADR-0031 d9). The parameter
    bounds, no parameter sent more than once among them, are checked first, before any fetch. A
    parameter past its bound (a repeated one and a present code_challenge that does not match the
    pattern included), a refused client or a refused redirect URI is a page, never a redirect
    (request.validation, oauth.client_invalid, oauth.redirect_refused); any later refusal,
    oauth.pkce_required for an absent code_challenge or an absent or non-S256 code_challenge_method
    among them, redirects with the RFC error, state and iss (RFC 9207). The authorization request writes
    no row: the page carries it as a handle sealed by an AEAD under a key the lead holds only in memory,
    so opening pages is not capped, and a full document cache evicts rather than refuses (ADR-0031 d5,
    d9). The page sets no cookie and carries the headers ADR-0031 d9 fixes: a Content-Security-Policy
    with default-src none, form-action self plus the redirect origin and frame-ancestors none, Cache-
    Control no-store, Referrer-Policy same-origin (no-referrer would make the browser send Origin: null
    on the page's own POST), X-Content-Type-Options nosniff.

    Args:
        response_type (AuthorizeResponseType):
        client_id (str):
        redirect_uri (str | Unset):
        code_challenge (str | Unset):
        code_challenge_method (AuthorizeCodeChallengeMethod | Unset):
        state (str | Unset):
        resource (str | Unset):
        scope (AuthorizeScope | Unset):
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Any | str
    """

    return (
        await asyncio_detailed(
            client=client,
            response_type=response_type,
            client_id=client_id,
            redirect_uri=redirect_uri,
            code_challenge=code_challenge,
            code_challenge_method=code_challenge_method,
            state=state,
            resource=resource,
            scope=scope,
            x_request_id=x_request_id,
            x_correlation_id=x_correlation_id,
            traceparent=traceparent,
        )
    ).parsed
