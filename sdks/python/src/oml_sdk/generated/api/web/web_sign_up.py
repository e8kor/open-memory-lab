from http import HTTPStatus
from typing import Any, cast

import httpx

from ... import errors
from ...client import AuthenticatedClient, Client
from ...models.web_sign_up_body import WebSignUpBody
from ...types import Response


def _get_kwargs(
    *,
    body: WebSignUpBody,
) -> dict[str, Any]:
    headers: dict[str, Any] = {}

    _kwargs: dict[str, Any] = {
        "method": "post",
        "url": "/web/sign-up",
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

    if response.status_code == 400:
        response_400 = response.text
        return response_400

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
    *,
    client: AuthenticatedClient | Client,
    body: WebSignUpBody,
) -> Response[Any | str]:
    """Accept the terms and create the account, in open sign-up.

     Only where signup.mode is open (ADR-0068 d2, d9). The same-origin check; the sealed __Host-
    oml_signup cookie the callback set for an unlisted, verified identity; accept_terms=yes and the
    current terms_version. Takes one slot of the client address's hourly limit (limit.rate past it).
    Creates the entry under a drawn user, journalled as web.signed_up with the terms version, issues
    <user>-web and <user>-review, opens a session and answers 303 /web/start. Proposed.

    Args:
        body (WebSignUpBody):

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
    body: WebSignUpBody,
) -> Any | str | None:
    """Accept the terms and create the account, in open sign-up.

     Only where signup.mode is open (ADR-0068 d2, d9). The same-origin check; the sealed __Host-
    oml_signup cookie the callback set for an unlisted, verified identity; accept_terms=yes and the
    current terms_version. Takes one slot of the client address's hourly limit (limit.rate past it).
    Creates the entry under a drawn user, journalled as web.signed_up with the terms version, issues
    <user>-web and <user>-review, opens a session and answers 303 /web/start. Proposed.

    Args:
        body (WebSignUpBody):

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
    body: WebSignUpBody,
) -> Response[Any | str]:
    """Accept the terms and create the account, in open sign-up.

     Only where signup.mode is open (ADR-0068 d2, d9). The same-origin check; the sealed __Host-
    oml_signup cookie the callback set for an unlisted, verified identity; accept_terms=yes and the
    current terms_version. Takes one slot of the client address's hourly limit (limit.rate past it).
    Creates the entry under a drawn user, journalled as web.signed_up with the terms version, issues
    <user>-web and <user>-review, opens a session and answers 303 /web/start. Proposed.

    Args:
        body (WebSignUpBody):

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
    body: WebSignUpBody,
) -> Any | str | None:
    """Accept the terms and create the account, in open sign-up.

     Only where signup.mode is open (ADR-0068 d2, d9). The same-origin check; the sealed __Host-
    oml_signup cookie the callback set for an unlisted, verified identity; accept_terms=yes and the
    current terms_version. Takes one slot of the client address's hourly limit (limit.rate past it).
    Creates the entry under a drawn user, journalled as web.signed_up with the terms version, issues
    <user>-web and <user>-review, opens a session and answers 303 /web/start. Proposed.

    Args:
        body (WebSignUpBody):

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
