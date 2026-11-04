from http import HTTPStatus
from typing import Any, cast
from urllib.parse import quote

import httpx

from ... import errors
from ...client import AuthenticatedClient, Client
from ...models.web_callback_provider import WebCallbackProvider
from ...types import UNSET, Response, Unset


def _get_kwargs(
    provider: WebCallbackProvider,
    *,
    code: str | Unset = UNSET,
    state: str | Unset = UNSET,
) -> dict[str, Any]:

    params: dict[str, Any] = {}

    params["code"] = code

    params["state"] = state

    params = {k: v for k, v in params.items() if v is not UNSET and v is not None}

    _kwargs: dict[str, Any] = {
        "method": "get",
        "url": "/web/callback/{provider}".format(
            provider=quote(str(provider), safe=""),
        ),
        "params": params,
    }

    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Any | str | None:
    if response.status_code == 303:
        response_303 = cast(Any, None)
        return response_303

    if response.status_code == 401:
        response_401 = response.text
        return response_401

    if response.status_code == 429:
        response_429 = response.text
        return response_429

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
    provider: WebCallbackProvider,
    *,
    client: AuthenticatedClient | Client,
    code: str | Unset = UNSET,
    state: str | Unset = UNSET,
) -> Response[Any | str]:
    """The provider's redirect; admits a listed, verified email and starts a session.

     Refused before any outbound call unless the sealed cookie opens, is under ten minutes old, names
    this provider and carries the query's state. Google: the ID token from the token endpoint over TLS,
    iss, aud, exp, nonce and email_verified checked. GitHub: the primary verified address of
    /user/emails. The provider subject is pinned per entry at first sign-in. Refusals are pages,
    auth.invalid with details.reason not_allowed, email_unverified, state_mismatch, identity_changed or
    provider_refused; limit.rate past the provider-call cap. The query is never logged (ADR-0045 d3, d4,
    d5, d14). Since v0.5.0.

    Args:
        provider (WebCallbackProvider):
        code (str | Unset):
        state (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Any | str]
    """

    kwargs = _get_kwargs(
        provider=provider,
        code=code,
        state=state,
    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    provider: WebCallbackProvider,
    *,
    client: AuthenticatedClient | Client,
    code: str | Unset = UNSET,
    state: str | Unset = UNSET,
) -> Any | str | None:
    """The provider's redirect; admits a listed, verified email and starts a session.

     Refused before any outbound call unless the sealed cookie opens, is under ten minutes old, names
    this provider and carries the query's state. Google: the ID token from the token endpoint over TLS,
    iss, aud, exp, nonce and email_verified checked. GitHub: the primary verified address of
    /user/emails. The provider subject is pinned per entry at first sign-in. Refusals are pages,
    auth.invalid with details.reason not_allowed, email_unverified, state_mismatch, identity_changed or
    provider_refused; limit.rate past the provider-call cap. The query is never logged (ADR-0045 d3, d4,
    d5, d14). Since v0.5.0.

    Args:
        provider (WebCallbackProvider):
        code (str | Unset):
        state (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Any | str
    """

    return sync_detailed(
        provider=provider,
        client=client,
        code=code,
        state=state,
    ).parsed


async def asyncio_detailed(
    provider: WebCallbackProvider,
    *,
    client: AuthenticatedClient | Client,
    code: str | Unset = UNSET,
    state: str | Unset = UNSET,
) -> Response[Any | str]:
    """The provider's redirect; admits a listed, verified email and starts a session.

     Refused before any outbound call unless the sealed cookie opens, is under ten minutes old, names
    this provider and carries the query's state. Google: the ID token from the token endpoint over TLS,
    iss, aud, exp, nonce and email_verified checked. GitHub: the primary verified address of
    /user/emails. The provider subject is pinned per entry at first sign-in. Refusals are pages,
    auth.invalid with details.reason not_allowed, email_unverified, state_mismatch, identity_changed or
    provider_refused; limit.rate past the provider-call cap. The query is never logged (ADR-0045 d3, d4,
    d5, d14). Since v0.5.0.

    Args:
        provider (WebCallbackProvider):
        code (str | Unset):
        state (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Any | str]
    """

    kwargs = _get_kwargs(
        provider=provider,
        code=code,
        state=state,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    provider: WebCallbackProvider,
    *,
    client: AuthenticatedClient | Client,
    code: str | Unset = UNSET,
    state: str | Unset = UNSET,
) -> Any | str | None:
    """The provider's redirect; admits a listed, verified email and starts a session.

     Refused before any outbound call unless the sealed cookie opens, is under ten minutes old, names
    this provider and carries the query's state. Google: the ID token from the token endpoint over TLS,
    iss, aud, exp, nonce and email_verified checked. GitHub: the primary verified address of
    /user/emails. The provider subject is pinned per entry at first sign-in. Refusals are pages,
    auth.invalid with details.reason not_allowed, email_unverified, state_mismatch, identity_changed or
    provider_refused; limit.rate past the provider-call cap. The query is never logged (ADR-0045 d3, d4,
    d5, d14). Since v0.5.0.

    Args:
        provider (WebCallbackProvider):
        code (str | Unset):
        state (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Any | str
    """

    return (
        await asyncio_detailed(
            provider=provider,
            client=client,
            code=code,
            state=state,
        )
    ).parsed
