from http import HTTPStatus
from typing import Any

import httpx

from ... import errors
from ...client import AuthenticatedClient, Client
from ...models.start_status_response_200 import StartStatusResponse200
from ...types import Response


def _get_kwargs() -> dict[str, Any]:

    _kwargs: dict[str, Any] = {
        "method": "get",
        "url": "/web/start/status",
    }

    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> StartStatusResponse200 | str | None:
    if response.status_code == 200:
        response_200 = StartStatusResponse200.from_dict(response.json())

        return response_200

    if response.status_code == 404:
        response_404 = response.text
        return response_404

    if client.raise_on_unexpected_status:
        raise errors.UnexpectedStatus(response.status_code, response.content)
    else:
        return None


def _build_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Response[StartStatusResponse200 | str]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    *,
    client: AuthenticatedClient | Client,
) -> Response[StartStatusResponse200 | str]:
    """Whether the person's agent has connected and the state of their newest memory.

     A session only; without one access.not_found (ADR-0068 d7, d10). connected is true once any of the
    person's agent keys authenticated since the process started or a connector key has an open
    connection. Proposed.

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[StartStatusResponse200 | str]
    """

    kwargs = _get_kwargs()

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    *,
    client: AuthenticatedClient | Client,
) -> StartStatusResponse200 | str | None:
    """Whether the person's agent has connected and the state of their newest memory.

     A session only; without one access.not_found (ADR-0068 d7, d10). connected is true once any of the
    person's agent keys authenticated since the process started or a connector key has an open
    connection. Proposed.

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        StartStatusResponse200 | str
    """

    return sync_detailed(
        client=client,
    ).parsed


async def asyncio_detailed(
    *,
    client: AuthenticatedClient | Client,
) -> Response[StartStatusResponse200 | str]:
    """Whether the person's agent has connected and the state of their newest memory.

     A session only; without one access.not_found (ADR-0068 d7, d10). connected is true once any of the
    person's agent keys authenticated since the process started or a connector key has an open
    connection. Proposed.

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[StartStatusResponse200 | str]
    """

    kwargs = _get_kwargs()

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    *,
    client: AuthenticatedClient | Client,
) -> StartStatusResponse200 | str | None:
    """Whether the person's agent has connected and the state of their newest memory.

     A session only; without one access.not_found (ADR-0068 d7, d10). connected is true once any of the
    person's agent keys authenticated since the process started or a connector key has an open
    connection. Proposed.

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        StartStatusResponse200 | str
    """

    return (
        await asyncio_detailed(
            client=client,
        )
    ).parsed
