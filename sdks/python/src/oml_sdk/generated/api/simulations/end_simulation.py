from http import HTTPStatus
from typing import Any
from urllib.parse import quote

import httpx

from ...client import AuthenticatedClient, Client
from ...models.act_result import ActResult
from ...models.end_simulation_body import EndSimulationBody
from ...models.error_envelope import ErrorEnvelope
from ...types import UNSET, Response, Unset


def _get_kwargs(
    simulation_id: str,
    *,
    body: EndSimulationBody | Unset = UNSET,
    idempotency_key: str,
) -> dict[str, Any]:
    headers: dict[str, Any] = {}
    headers["Idempotency-Key"] = idempotency_key

    _kwargs: dict[str, Any] = {
        "method": "post",
        "url": "/v2/simulations/{simulation_id}/end".format(
            simulation_id=quote(str(simulation_id), safe=""),
        ),
    }

    if not isinstance(body, Unset):
        _kwargs["json"] = body.to_dict()

    headers["Content-Type"] = "application/json"

    _kwargs["headers"] = headers
    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> ActResult | ErrorEnvelope:
    if response.status_code == 202:
        response_202 = ActResult.from_dict(response.json())

        return response_202

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
) -> Response[ActResult | ErrorEnvelope]:
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
    body: EndSimulationBody | Unset = UNSET,
    idempotency_key: str,
) -> Response[ActResult | ErrorEnvelope]:
    """Journalled end — demotes the simulation's rows and triggers re-review of tainted claims; dry_run
    lists everything the end would touch.

     Requires: admin key role.

    Args:
        simulation_id (str):
        idempotency_key (str):
        body (EndSimulationBody | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ActResult | ErrorEnvelope]
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
    body: EndSimulationBody | Unset = UNSET,
    idempotency_key: str,
) -> ActResult | ErrorEnvelope | None:
    """Journalled end — demotes the simulation's rows and triggers re-review of tainted claims; dry_run
    lists everything the end would touch.

     Requires: admin key role.

    Args:
        simulation_id (str):
        idempotency_key (str):
        body (EndSimulationBody | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ActResult | ErrorEnvelope
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
    body: EndSimulationBody | Unset = UNSET,
    idempotency_key: str,
) -> Response[ActResult | ErrorEnvelope]:
    """Journalled end — demotes the simulation's rows and triggers re-review of tainted claims; dry_run
    lists everything the end would touch.

     Requires: admin key role.

    Args:
        simulation_id (str):
        idempotency_key (str):
        body (EndSimulationBody | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ActResult | ErrorEnvelope]
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
    body: EndSimulationBody | Unset = UNSET,
    idempotency_key: str,
) -> ActResult | ErrorEnvelope | None:
    """Journalled end — demotes the simulation's rows and triggers re-review of tainted claims; dry_run
    lists everything the end would touch.

     Requires: admin key role.

    Args:
        simulation_id (str):
        idempotency_key (str):
        body (EndSimulationBody | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ActResult | ErrorEnvelope
    """

    return (
        await asyncio_detailed(
            simulation_id=simulation_id,
            client=client,
            body=body,
            idempotency_key=idempotency_key,
        )
    ).parsed
