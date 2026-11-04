from http import HTTPStatus
from typing import Any
from urllib.parse import quote

import httpx

from ...client import AuthenticatedClient, Client
from ...models.crew import Crew
from ...models.crew_update import CrewUpdate
from ...models.error_envelope import ErrorEnvelope
from ...types import UNSET, Response, Unset


def _get_kwargs(
    crew: str,
    *,
    body: CrewUpdate,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
    idempotency_key: str,
) -> dict[str, Any]:
    headers: dict[str, Any] = {}
    if not isinstance(x_request_id, Unset):
        headers["X-Request-Id"] = x_request_id

    if not isinstance(x_correlation_id, Unset):
        headers["X-Correlation-Id"] = x_correlation_id

    if not isinstance(traceparent, Unset):
        headers["traceparent"] = traceparent

    headers["Idempotency-Key"] = idempotency_key

    _kwargs: dict[str, Any] = {
        "method": "post",
        "url": "/v2/crews/{crew}/update".format(
            crew=quote(str(crew), safe=""),
        ),
    }

    _kwargs["json"] = body.to_dict()

    headers["Content-Type"] = "application/json"

    _kwargs["headers"] = headers
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

    if response.status_code == 415:
        response_415 = ErrorEnvelope.from_dict(response.json())

        return response_415

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
    crew: str,
    *,
    client: AuthenticatedClient | Client,
    body: CrewUpdate,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
    idempotency_key: str,
) -> Response[Crew | ErrorEnvelope]:
    """Journalled crew-record update (crew.updated) — charter URI and external manager; the name never
    changes.

     Requires: admin key role. A crew record has no status, so the update is admitted whatever the status
    of the workareas that name it (ADR-0026 d8).

    Args:
        crew (str): A crew record, by its id or its name, resolved server-side like every
            reference (ADR-0026 d4).
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        idempotency_key (str):
        body (CrewUpdate): At least one field; the name is not updatable.

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Crew | ErrorEnvelope]
    """

    kwargs = _get_kwargs(
        crew=crew,
        body=body,
        x_request_id=x_request_id,
        x_correlation_id=x_correlation_id,
        traceparent=traceparent,
        idempotency_key=idempotency_key,
    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    crew: str,
    *,
    client: AuthenticatedClient | Client,
    body: CrewUpdate,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
    idempotency_key: str,
) -> Crew | ErrorEnvelope | None:
    """Journalled crew-record update (crew.updated) — charter URI and external manager; the name never
    changes.

     Requires: admin key role. A crew record has no status, so the update is admitted whatever the status
    of the workareas that name it (ADR-0026 d8).

    Args:
        crew (str): A crew record, by its id or its name, resolved server-side like every
            reference (ADR-0026 d4).
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        idempotency_key (str):
        body (CrewUpdate): At least one field; the name is not updatable.

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Crew | ErrorEnvelope
    """

    return sync_detailed(
        crew=crew,
        client=client,
        body=body,
        x_request_id=x_request_id,
        x_correlation_id=x_correlation_id,
        traceparent=traceparent,
        idempotency_key=idempotency_key,
    ).parsed


async def asyncio_detailed(
    crew: str,
    *,
    client: AuthenticatedClient | Client,
    body: CrewUpdate,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
    idempotency_key: str,
) -> Response[Crew | ErrorEnvelope]:
    """Journalled crew-record update (crew.updated) — charter URI and external manager; the name never
    changes.

     Requires: admin key role. A crew record has no status, so the update is admitted whatever the status
    of the workareas that name it (ADR-0026 d8).

    Args:
        crew (str): A crew record, by its id or its name, resolved server-side like every
            reference (ADR-0026 d4).
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        idempotency_key (str):
        body (CrewUpdate): At least one field; the name is not updatable.

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Crew | ErrorEnvelope]
    """

    kwargs = _get_kwargs(
        crew=crew,
        body=body,
        x_request_id=x_request_id,
        x_correlation_id=x_correlation_id,
        traceparent=traceparent,
        idempotency_key=idempotency_key,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    crew: str,
    *,
    client: AuthenticatedClient | Client,
    body: CrewUpdate,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
    idempotency_key: str,
) -> Crew | ErrorEnvelope | None:
    """Journalled crew-record update (crew.updated) — charter URI and external manager; the name never
    changes.

     Requires: admin key role. A crew record has no status, so the update is admitted whatever the status
    of the workareas that name it (ADR-0026 d8).

    Args:
        crew (str): A crew record, by its id or its name, resolved server-side like every
            reference (ADR-0026 d4).
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        idempotency_key (str):
        body (CrewUpdate): At least one field; the name is not updatable.

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Crew | ErrorEnvelope
    """

    return (
        await asyncio_detailed(
            crew=crew,
            client=client,
            body=body,
            x_request_id=x_request_id,
            x_correlation_id=x_correlation_id,
            traceparent=traceparent,
            idempotency_key=idempotency_key,
        )
    ).parsed
