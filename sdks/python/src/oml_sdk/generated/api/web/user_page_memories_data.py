from http import HTTPStatus
from typing import Any

import httpx

from ... import errors
from ...client import AuthenticatedClient, Client
from ...models.error_envelope import ErrorEnvelope
from ...models.user_page_memories_data_response_200 import (
    UserPageMemoriesDataResponse200,
)
from ...types import Response


def _get_kwargs() -> dict[str, Any]:

    _kwargs: dict[str, Any] = {
        "method": "get",
        "url": "/web/data/memories",
    }

    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> ErrorEnvelope | UserPageMemoriesDataResponse200 | None:
    if response.status_code == 200:
        response_200 = UserPageMemoriesDataResponse200.from_dict(response.json())

        return response_200

    if response.status_code == 404:
        response_404 = ErrorEnvelope.from_dict(response.json())

        return response_404

    if client.raise_on_unexpected_status:
        raise errors.UnexpectedStatus(response.status_code, response.content)
    else:
        return None


def _build_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Response[ErrorEnvelope | UserPageMemoriesDataResponse200]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    *,
    client: AuthenticatedClient | Client,
) -> Response[ErrorEnvelope | UserPageMemoriesDataResponse200]:
    """The user page's page of memories as JSON, for the Atlas view.

     The Memories view's query (state, workarea, origin, author, q, limit, cursor) and its scope, as
    <user>-web: the rows the list shows with the card's fields alone, and the workareas the key may read
    under the filter with their rows counted by state and their dropdown's acts. The session cookie
    alone; an Authorization header is access.verb_ceiling web_is_human_only, no session
    access.not_found; no-store, nosniff, no CORS (ADR-0045 d25, d30).

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | UserPageMemoriesDataResponse200]
    """

    kwargs = _get_kwargs()

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    *,
    client: AuthenticatedClient | Client,
) -> ErrorEnvelope | UserPageMemoriesDataResponse200 | None:
    """The user page's page of memories as JSON, for the Atlas view.

     The Memories view's query (state, workarea, origin, author, q, limit, cursor) and its scope, as
    <user>-web: the rows the list shows with the card's fields alone, and the workareas the key may read
    under the filter with their rows counted by state and their dropdown's acts. The session cookie
    alone; an Authorization header is access.verb_ceiling web_is_human_only, no session
    access.not_found; no-store, nosniff, no CORS (ADR-0045 d25, d30).

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | UserPageMemoriesDataResponse200
    """

    return sync_detailed(
        client=client,
    ).parsed


async def asyncio_detailed(
    *,
    client: AuthenticatedClient | Client,
) -> Response[ErrorEnvelope | UserPageMemoriesDataResponse200]:
    """The user page's page of memories as JSON, for the Atlas view.

     The Memories view's query (state, workarea, origin, author, q, limit, cursor) and its scope, as
    <user>-web: the rows the list shows with the card's fields alone, and the workareas the key may read
    under the filter with their rows counted by state and their dropdown's acts. The session cookie
    alone; an Authorization header is access.verb_ceiling web_is_human_only, no session
    access.not_found; no-store, nosniff, no CORS (ADR-0045 d25, d30).

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | UserPageMemoriesDataResponse200]
    """

    kwargs = _get_kwargs()

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    *,
    client: AuthenticatedClient | Client,
) -> ErrorEnvelope | UserPageMemoriesDataResponse200 | None:
    """The user page's page of memories as JSON, for the Atlas view.

     The Memories view's query (state, workarea, origin, author, q, limit, cursor) and its scope, as
    <user>-web: the rows the list shows with the card's fields alone, and the workareas the key may read
    under the filter with their rows counted by state and their dropdown's acts. The session cookie
    alone; an Authorization header is access.verb_ceiling web_is_human_only, no session
    access.not_found; no-store, nosniff, no CORS (ADR-0045 d25, d30).

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | UserPageMemoriesDataResponse200
    """

    return (
        await asyncio_detailed(
            client=client,
        )
    ).parsed
