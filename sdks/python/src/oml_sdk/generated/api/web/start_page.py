from http import HTTPStatus
from typing import Any

import httpx

from ... import errors
from ...client import AuthenticatedClient, Client
from ...models.start_page_client import StartPageClient
from ...types import UNSET, Response, Unset


def _get_kwargs(
    *,
    client_query: StartPageClient | Unset = UNSET,
) -> dict[str, Any]:

    params: dict[str, Any] = {}

    json_client_query: str | Unset = UNSET
    if not isinstance(client_query, Unset):
        json_client_query = client_query.value

    params["client"] = json_client_query

    params = {k: v for k, v in params.items() if v is not UNSET and v is not None}

    _kwargs: dict[str, Any] = {
        "method": "get",
        "url": "/web/start",
        "params": params,
    }

    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> str | None:
    if response.status_code == 200:
        response_200 = response.text
        return response_200

    if client.raise_on_unexpected_status:
        raise errors.UnexpectedStatus(response.status_code, response.content)
    else:
        return None


def _build_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Response[str]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    *,
    client: AuthenticatedClient | Client,
    client_query: StartPageClient | Unset = UNSET,
) -> Response[str]:
    """The onboarding page; the sign-in page without a session.

     Three steps: choose a client (query client: claude-ai, claude-code, other), get its credential, and
    wait for the first connection and the first memory. The only page with a script, hashed in its CSP
    with connect-src self, for its copy buttons and its status poll (ADR-0068 d10). Proposed.

    Args:
        client_query (StartPageClient | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[str]
    """

    kwargs = _get_kwargs(
        client_query=client_query,
    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    *,
    client: AuthenticatedClient | Client,
    client_query: StartPageClient | Unset = UNSET,
) -> str | None:
    """The onboarding page; the sign-in page without a session.

     Three steps: choose a client (query client: claude-ai, claude-code, other), get its credential, and
    wait for the first connection and the first memory. The only page with a script, hashed in its CSP
    with connect-src self, for its copy buttons and its status poll (ADR-0068 d10). Proposed.

    Args:
        client_query (StartPageClient | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        str
    """

    return sync_detailed(
        client=client,
        client_query=client_query,
    ).parsed


async def asyncio_detailed(
    *,
    client: AuthenticatedClient | Client,
    client_query: StartPageClient | Unset = UNSET,
) -> Response[str]:
    """The onboarding page; the sign-in page without a session.

     Three steps: choose a client (query client: claude-ai, claude-code, other), get its credential, and
    wait for the first connection and the first memory. The only page with a script, hashed in its CSP
    with connect-src self, for its copy buttons and its status poll (ADR-0068 d10). Proposed.

    Args:
        client_query (StartPageClient | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[str]
    """

    kwargs = _get_kwargs(
        client_query=client_query,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    *,
    client: AuthenticatedClient | Client,
    client_query: StartPageClient | Unset = UNSET,
) -> str | None:
    """The onboarding page; the sign-in page without a session.

     Three steps: choose a client (query client: claude-ai, claude-code, other), get its credential, and
    wait for the first connection and the first memory. The only page with a script, hashed in its CSP
    with connect-src self, for its copy buttons and its status poll (ADR-0068 d10). Proposed.

    Args:
        client_query (StartPageClient | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        str
    """

    return (
        await asyncio_detailed(
            client=client,
            client_query=client_query,
        )
    ).parsed
