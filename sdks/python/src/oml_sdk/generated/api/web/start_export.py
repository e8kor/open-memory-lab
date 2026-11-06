from http import HTTPStatus
from typing import Any

import httpx

from ... import errors
from ...client import AuthenticatedClient, Client
from ...models.start_export_response_200 import StartExportResponse200
from ...types import Response


def _get_kwargs() -> dict[str, Any]:

    _kwargs: dict[str, Any] = {
        "method": "get",
        "url": "/web/start/export",
    }

    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> StartExportResponse200 | str | None:
    if response.status_code == 200:
        response_200 = StartExportResponse200.from_dict(response.json())

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
) -> Response[StartExportResponse200 | str]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    *,
    client: AuthenticatedClient | Client,
) -> Response[StartExportResponse200 | str]:
    """Everything the person holds, as a JSON download.

     A session only. Every memory in the person's root and below, every state, with its workarea path,
    claim key, version, state, text and times; the person's keys (ADR-0068 d6). Proposed.

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[StartExportResponse200 | str]
    """

    kwargs = _get_kwargs()

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    *,
    client: AuthenticatedClient | Client,
) -> StartExportResponse200 | str | None:
    """Everything the person holds, as a JSON download.

     A session only. Every memory in the person's root and below, every state, with its workarea path,
    claim key, version, state, text and times; the person's keys (ADR-0068 d6). Proposed.

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        StartExportResponse200 | str
    """

    return sync_detailed(
        client=client,
    ).parsed


async def asyncio_detailed(
    *,
    client: AuthenticatedClient | Client,
) -> Response[StartExportResponse200 | str]:
    """Everything the person holds, as a JSON download.

     A session only. Every memory in the person's root and below, every state, with its workarea path,
    claim key, version, state, text and times; the person's keys (ADR-0068 d6). Proposed.

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[StartExportResponse200 | str]
    """

    kwargs = _get_kwargs()

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    *,
    client: AuthenticatedClient | Client,
) -> StartExportResponse200 | str | None:
    """Everything the person holds, as a JSON download.

     A session only. Every memory in the person's root and below, every state, with its workarea path,
    claim key, version, state, text and times; the person's keys (ADR-0068 d6). Proposed.

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        StartExportResponse200 | str
    """

    return (
        await asyncio_detailed(
            client=client,
        )
    ).parsed
