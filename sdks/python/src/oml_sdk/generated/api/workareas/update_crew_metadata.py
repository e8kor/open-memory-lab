from http import HTTPStatus
from typing import Any
from urllib.parse import quote

import httpx

from ...client import AuthenticatedClient, Client
from ...models.crew_record import CrewRecord
from ...models.error_envelope import ErrorEnvelope
from ...types import Response


def _get_kwargs(
    workarea_id: str,
    *,
    body: CrewRecord,
    idempotency_key: str,
) -> dict[str, Any]:
    headers: dict[str, Any] = {}
    headers["Idempotency-Key"] = idempotency_key

    _kwargs: dict[str, Any] = {
        "method": "post",
        "url": "/v2/workareas/{workarea_id}/crew".format(
            workarea_id=quote(str(workarea_id), safe=""),
        ),
    }

    _kwargs["json"] = body.to_dict()

    headers["Content-Type"] = "application/json"

    _kwargs["headers"] = headers
    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> ErrorEnvelope:
    if response.status_code == 415:
        response_415 = ErrorEnvelope.from_dict(response.json())

        return response_415

    response_default = ErrorEnvelope.from_dict(response.json())

    return response_default


def _build_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Response[ErrorEnvelope]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    workarea_id: str,
    *,
    client: AuthenticatedClient | Client,
    body: CrewRecord,
    idempotency_key: str,
) -> Response[ErrorEnvelope]:
    """Journalled crew-metadata update; allowed on validated areas (freeze covers claims, not metadata).

     Requires: admin key role.

    Args:
        workarea_id (str):
        idempotency_key (str):
        body (CrewRecord): Recorded, not operated — OML never contacts the external manager.

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope]
    """

    kwargs = _get_kwargs(
        workarea_id=workarea_id,
        body=body,
        idempotency_key=idempotency_key,
    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    workarea_id: str,
    *,
    client: AuthenticatedClient | Client,
    body: CrewRecord,
    idempotency_key: str,
) -> ErrorEnvelope | None:
    """Journalled crew-metadata update; allowed on validated areas (freeze covers claims, not metadata).

     Requires: admin key role.

    Args:
        workarea_id (str):
        idempotency_key (str):
        body (CrewRecord): Recorded, not operated — OML never contacts the external manager.

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope
    """

    return sync_detailed(
        workarea_id=workarea_id,
        client=client,
        body=body,
        idempotency_key=idempotency_key,
    ).parsed


async def asyncio_detailed(
    workarea_id: str,
    *,
    client: AuthenticatedClient | Client,
    body: CrewRecord,
    idempotency_key: str,
) -> Response[ErrorEnvelope]:
    """Journalled crew-metadata update; allowed on validated areas (freeze covers claims, not metadata).

     Requires: admin key role.

    Args:
        workarea_id (str):
        idempotency_key (str):
        body (CrewRecord): Recorded, not operated — OML never contacts the external manager.

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope]
    """

    kwargs = _get_kwargs(
        workarea_id=workarea_id,
        body=body,
        idempotency_key=idempotency_key,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    workarea_id: str,
    *,
    client: AuthenticatedClient | Client,
    body: CrewRecord,
    idempotency_key: str,
) -> ErrorEnvelope | None:
    """Journalled crew-metadata update; allowed on validated areas (freeze covers claims, not metadata).

     Requires: admin key role.

    Args:
        workarea_id (str):
        idempotency_key (str):
        body (CrewRecord): Recorded, not operated — OML never contacts the external manager.

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope
    """

    return (
        await asyncio_detailed(
            workarea_id=workarea_id,
            client=client,
            body=body,
            idempotency_key=idempotency_key,
        )
    ).parsed
