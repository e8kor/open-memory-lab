from http import HTTPStatus
from typing import Any
from urllib.parse import quote

import httpx

from ...client import AuthenticatedClient, Client
from ...models.change_simulation_audience_body import ChangeSimulationAudienceBody
from ...models.error_envelope import ErrorEnvelope
from ...models.simulation import Simulation
from ...types import Response


def _get_kwargs(
    simulation_id: str,
    *,
    body: ChangeSimulationAudienceBody,
    idempotency_key: str,
) -> dict[str, Any]:
    headers: dict[str, Any] = {}
    headers["Idempotency-Key"] = idempotency_key

    _kwargs: dict[str, Any] = {
        "method": "post",
        "url": "/v2/simulations/{simulation_id}/audience".format(
            simulation_id=quote(str(simulation_id), safe=""),
        ),
    }

    _kwargs["json"] = body.to_dict()

    headers["Content-Type"] = "application/json"

    _kwargs["headers"] = headers
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

    if response.status_code == 409:
        response_409 = ErrorEnvelope.from_dict(response.json())

        return response_409

    if response.status_code == 415:
        response_415 = ErrorEnvelope.from_dict(response.json())

        return response_415

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
    body: ChangeSimulationAudienceBody,
    idempotency_key: str,
) -> Response[ErrorEnvelope | Simulation]:
    """Correct a live simulation's audience — the only recovery from the fail-closed no-dreamer state short
    of ending it.

     Requires: admin key role. Journals simulation.audience_changed; the audience at any seq is the fold
    of simulation.opened plus every audience_changed since. Removal takes effect on the next read; past
    reads stand in the read log.

    Args:
        simulation_id (str):
        idempotency_key (str):
        body (ChangeSimulationAudienceBody): At least one of add/remove must be non-empty
            (request.validation otherwise).

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | Simulation]
    """

    kwargs = _get_kwargs(
        simulation_id=simulation_id,
        body=body,
        idempotency_key=idempotency_key,
    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    simulation_id: str,
    *,
    client: AuthenticatedClient | Client,
    body: ChangeSimulationAudienceBody,
    idempotency_key: str,
) -> ErrorEnvelope | Simulation | None:
    """Correct a live simulation's audience — the only recovery from the fail-closed no-dreamer state short
    of ending it.

     Requires: admin key role. Journals simulation.audience_changed; the audience at any seq is the fold
    of simulation.opened plus every audience_changed since. Removal takes effect on the next read; past
    reads stand in the read log.

    Args:
        simulation_id (str):
        idempotency_key (str):
        body (ChangeSimulationAudienceBody): At least one of add/remove must be non-empty
            (request.validation otherwise).

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | Simulation
    """

    return sync_detailed(
        simulation_id=simulation_id,
        client=client,
        body=body,
        idempotency_key=idempotency_key,
    ).parsed


async def asyncio_detailed(
    simulation_id: str,
    *,
    client: AuthenticatedClient | Client,
    body: ChangeSimulationAudienceBody,
    idempotency_key: str,
) -> Response[ErrorEnvelope | Simulation]:
    """Correct a live simulation's audience — the only recovery from the fail-closed no-dreamer state short
    of ending it.

     Requires: admin key role. Journals simulation.audience_changed; the audience at any seq is the fold
    of simulation.opened plus every audience_changed since. Removal takes effect on the next read; past
    reads stand in the read log.

    Args:
        simulation_id (str):
        idempotency_key (str):
        body (ChangeSimulationAudienceBody): At least one of add/remove must be non-empty
            (request.validation otherwise).

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | Simulation]
    """

    kwargs = _get_kwargs(
        simulation_id=simulation_id,
        body=body,
        idempotency_key=idempotency_key,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    simulation_id: str,
    *,
    client: AuthenticatedClient | Client,
    body: ChangeSimulationAudienceBody,
    idempotency_key: str,
) -> ErrorEnvelope | Simulation | None:
    """Correct a live simulation's audience — the only recovery from the fail-closed no-dreamer state short
    of ending it.

     Requires: admin key role. Journals simulation.audience_changed; the audience at any seq is the fold
    of simulation.opened plus every audience_changed since. Removal takes effect on the next read; past
    reads stand in the read log.

    Args:
        simulation_id (str):
        idempotency_key (str):
        body (ChangeSimulationAudienceBody): At least one of add/remove must be non-empty
            (request.validation otherwise).

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
            body=body,
            idempotency_key=idempotency_key,
        )
    ).parsed
