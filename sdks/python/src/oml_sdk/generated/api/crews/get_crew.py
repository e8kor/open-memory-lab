from http import HTTPStatus
from typing import Any
from urllib.parse import quote

import httpx

from ...client import AuthenticatedClient, Client
from ...models.crew import Crew
from ...models.error_envelope import ErrorEnvelope
from ...types import Response


def _get_kwargs(
    crew_id: str,
) -> dict[str, Any]:

    _kwargs: dict[str, Any] = {
        "method": "get",
        "url": "/v2/crews/{crew_id}".format(
            crew_id=quote(str(crew_id), safe=""),
        ),
    }

    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Crew | ErrorEnvelope:
    if response.status_code == 200:
        response_200 = Crew.from_dict(response.json())

        return response_200

    if response.status_code == 404:
        response_404 = ErrorEnvelope.from_dict(response.json())

        return response_404

    response_default = ErrorEnvelope.from_dict(response.json())

    return response_default


def _build_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Response[Crew | ErrorEnvelope]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    crew_id: str,
    *,
    client: AuthenticatedClient | Client,
) -> Response[Crew | ErrorEnvelope]:
    """One crew record, by id or name.

     Requires: admin key role.

    Args:
        crew_id (str): A crew record, by its id or its name, resolved server-side like every
            reference (ADR-0026 d4).

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Crew | ErrorEnvelope]
    """

    kwargs = _get_kwargs(
        crew_id=crew_id,
    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    crew_id: str,
    *,
    client: AuthenticatedClient | Client,
) -> Crew | ErrorEnvelope | None:
    """One crew record, by id or name.

     Requires: admin key role.

    Args:
        crew_id (str): A crew record, by its id or its name, resolved server-side like every
            reference (ADR-0026 d4).

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Crew | ErrorEnvelope
    """

    return sync_detailed(
        crew_id=crew_id,
        client=client,
    ).parsed


async def asyncio_detailed(
    crew_id: str,
    *,
    client: AuthenticatedClient | Client,
) -> Response[Crew | ErrorEnvelope]:
    """One crew record, by id or name.

     Requires: admin key role.

    Args:
        crew_id (str): A crew record, by its id or its name, resolved server-side like every
            reference (ADR-0026 d4).

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Crew | ErrorEnvelope]
    """

    kwargs = _get_kwargs(
        crew_id=crew_id,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    crew_id: str,
    *,
    client: AuthenticatedClient | Client,
) -> Crew | ErrorEnvelope | None:
    """One crew record, by id or name.

     Requires: admin key role.

    Args:
        crew_id (str): A crew record, by its id or its name, resolved server-side like every
            reference (ADR-0026 d4).

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Crew | ErrorEnvelope
    """

    return (
        await asyncio_detailed(
            crew_id=crew_id,
            client=client,
        )
    ).parsed
