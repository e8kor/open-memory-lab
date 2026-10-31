from http import HTTPStatus
from typing import Any
from urllib.parse import quote

import httpx

from ...client import AuthenticatedClient, Client
from ...models.error_envelope import ErrorEnvelope
from ...models.simulation import Simulation
from ...types import Response


def _get_kwargs(
    simulation_id: str,
) -> dict[str, Any]:

    _kwargs: dict[str, Any] = {
        "method": "get",
        "url": "/v2/simulations/{simulation_id}".format(
            simulation_id=quote(str(simulation_id), safe=""),
        ),
    }

    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> ErrorEnvelope | Simulation:
    if response.status_code == 200:
        response_200 = Simulation.from_dict(response.json())

        return response_200

    if response.status_code == 404:
        response_404 = ErrorEnvelope.from_dict(response.json())

        return response_404

    response_default = ErrorEnvelope.from_dict(response.json())

    return response_default


def _build_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Response[ErrorEnvelope | Simulation]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    simulation_id: str,
    *,
    client: AuthenticatedClient | Client,
) -> Response[ErrorEnvelope | Simulation]:
    """One simulation — audience, rows written, sim-derived taints outside it.

     Requires: admin key role.

    Args:
        simulation_id (str):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | Simulation]
    """

    kwargs = _get_kwargs(
        simulation_id=simulation_id,
    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    simulation_id: str,
    *,
    client: AuthenticatedClient | Client,
) -> ErrorEnvelope | Simulation | None:
    """One simulation — audience, rows written, sim-derived taints outside it.

     Requires: admin key role.

    Args:
        simulation_id (str):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | Simulation
    """

    return sync_detailed(
        simulation_id=simulation_id,
        client=client,
    ).parsed


async def asyncio_detailed(
    simulation_id: str,
    *,
    client: AuthenticatedClient | Client,
) -> Response[ErrorEnvelope | Simulation]:
    """One simulation — audience, rows written, sim-derived taints outside it.

     Requires: admin key role.

    Args:
        simulation_id (str):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | Simulation]
    """

    kwargs = _get_kwargs(
        simulation_id=simulation_id,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    simulation_id: str,
    *,
    client: AuthenticatedClient | Client,
) -> ErrorEnvelope | Simulation | None:
    """One simulation — audience, rows written, sim-derived taints outside it.

     Requires: admin key role.

    Args:
        simulation_id (str):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | Simulation
    """

    return (
        await asyncio_detailed(
            simulation_id=simulation_id,
            client=client,
        )
    ).parsed
