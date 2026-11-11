from http import HTTPStatus
from typing import Any, cast

import httpx

from ... import errors
from ...client import AuthenticatedClient, Client
from ...types import Response


def _get_kwargs() -> dict[str, Any]:

    _kwargs: dict[str, Any] = {
        "method": "get",
        "url": "/web",
    }

    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Any | str | None:
    if response.status_code == 200:
        response_200 = response.text
        return response_200

    if response.status_code == 303:
        response_303 = cast(Any, None)
        return response_303

    if response.status_code == 404:
        response_404 = response.text
        return response_404

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
) -> Response[Any | str]:
    """The sign-in page without a session; a signed-in browser goes on to the Keys view.

     Authenticated by the __Host-oml_session cookie alone; an Authorization header is refused with
    access.verb_ceiling, details.reason web_is_human_only (ADR-0045 d7). Every view of the user page is
    oml-web's at a path of its own (ADR-0070 d7 (b)); a signed-in request naming a view is
    access.not_found, the old ?view= URLs' 303s having been deleted in v0.7.1 (ADR-0070, "The old URLs
    deleted"). Served with the connect page's headers (ADR-0045 d8). Routed only on the lead and only
    where oauth.issuer and a sign-in provider are set. Since v0.5.0.

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Any | str]
    """

    kwargs = _get_kwargs()

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    *,
    client: AuthenticatedClient | Client,
) -> Any | str | None:
    """The sign-in page without a session; a signed-in browser goes on to the Keys view.

     Authenticated by the __Host-oml_session cookie alone; an Authorization header is refused with
    access.verb_ceiling, details.reason web_is_human_only (ADR-0045 d7). Every view of the user page is
    oml-web's at a path of its own (ADR-0070 d7 (b)); a signed-in request naming a view is
    access.not_found, the old ?view= URLs' 303s having been deleted in v0.7.1 (ADR-0070, "The old URLs
    deleted"). Served with the connect page's headers (ADR-0045 d8). Routed only on the lead and only
    where oauth.issuer and a sign-in provider are set. Since v0.5.0.

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Any | str
    """

    return sync_detailed(
        client=client,
    ).parsed


async def asyncio_detailed(
    *,
    client: AuthenticatedClient | Client,
) -> Response[Any | str]:
    """The sign-in page without a session; a signed-in browser goes on to the Keys view.

     Authenticated by the __Host-oml_session cookie alone; an Authorization header is refused with
    access.verb_ceiling, details.reason web_is_human_only (ADR-0045 d7). Every view of the user page is
    oml-web's at a path of its own (ADR-0070 d7 (b)); a signed-in request naming a view is
    access.not_found, the old ?view= URLs' 303s having been deleted in v0.7.1 (ADR-0070, "The old URLs
    deleted"). Served with the connect page's headers (ADR-0045 d8). Routed only on the lead and only
    where oauth.issuer and a sign-in provider are set. Since v0.5.0.

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Any | str]
    """

    kwargs = _get_kwargs()

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    *,
    client: AuthenticatedClient | Client,
) -> Any | str | None:
    """The sign-in page without a session; a signed-in browser goes on to the Keys view.

     Authenticated by the __Host-oml_session cookie alone; an Authorization header is refused with
    access.verb_ceiling, details.reason web_is_human_only (ADR-0045 d7). Every view of the user page is
    oml-web's at a path of its own (ADR-0070 d7 (b)); a signed-in request naming a view is
    access.not_found, the old ?view= URLs' 303s having been deleted in v0.7.1 (ADR-0070, "The old URLs
    deleted"). Served with the connect page's headers (ADR-0045 d8). Routed only on the lead and only
    where oauth.issuer and a sign-in provider are set. Since v0.5.0.

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Any | str
    """

    return (
        await asyncio_detailed(
            client=client,
        )
    ).parsed
