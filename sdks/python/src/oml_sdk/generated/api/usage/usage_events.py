from http import HTTPStatus
from typing import Any

import httpx

from ...client import AuthenticatedClient, Client
from ...models.error_envelope import ErrorEnvelope
from ...models.usage_events_response_200 import UsageEventsResponse200
from ...types import UNSET, Response, Unset


def _get_kwargs(
    *,
    limit: int | Unset = 50,
    before: str | Unset = UNSET,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> dict[str, Any]:
    headers: dict[str, Any] = {}
    if not isinstance(x_request_id, Unset):
        headers["X-Request-Id"] = x_request_id

    if not isinstance(x_correlation_id, Unset):
        headers["X-Correlation-Id"] = x_correlation_id

    if not isinstance(traceparent, Unset):
        headers["traceparent"] = traceparent

    params: dict[str, Any] = {}

    params["limit"] = limit

    params["before"] = before

    params = {k: v for k, v in params.items() if v is not UNSET and v is not None}

    _kwargs: dict[str, Any] = {
        "method": "get",
        "url": "/v2/usage/events",
        "params": params,
    }

    _kwargs["headers"] = headers
    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> ErrorEnvelope | UsageEventsResponse200:
    if response.status_code == 200:
        response_200 = UsageEventsResponse200.from_dict(response.json())

        return response_200

    if response.status_code == 403:
        response_403 = ErrorEnvelope.from_dict(response.json())

        return response_403

    response_default = ErrorEnvelope.from_dict(response.json())

    return response_default


def _build_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Response[ErrorEnvelope | UsageEventsResponse200]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    *,
    client: AuthenticatedClient | Client,
    limit: int | Unset = 50,
    before: str | Unset = UNSET,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> Response[ErrorEnvelope | UsageEventsResponse200]:
    """The tenant's recalls of the last 30 days, with what their readers did next, newest first.

     Requires: admin key role (ADR-0054 d2). No agent, writer, reader or dreamer key reads an event, on
    any transport. query_text is present only for a recall made while the tenant kept text. A row past
    the 30 days is never served, purged or not. system.unavailable with reason telemetry_off where the
    server records no recall. Since v0.5.7.

    Args:
        limit (int | Unset):  Default: 50.
        before (str | Unset):
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | UsageEventsResponse200]
    """

    kwargs = _get_kwargs(
        limit=limit,
        before=before,
        x_request_id=x_request_id,
        x_correlation_id=x_correlation_id,
        traceparent=traceparent,
    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    *,
    client: AuthenticatedClient | Client,
    limit: int | Unset = 50,
    before: str | Unset = UNSET,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> ErrorEnvelope | UsageEventsResponse200 | None:
    """The tenant's recalls of the last 30 days, with what their readers did next, newest first.

     Requires: admin key role (ADR-0054 d2). No agent, writer, reader or dreamer key reads an event, on
    any transport. query_text is present only for a recall made while the tenant kept text. A row past
    the 30 days is never served, purged or not. system.unavailable with reason telemetry_off where the
    server records no recall. Since v0.5.7.

    Args:
        limit (int | Unset):  Default: 50.
        before (str | Unset):
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | UsageEventsResponse200
    """

    return sync_detailed(
        client=client,
        limit=limit,
        before=before,
        x_request_id=x_request_id,
        x_correlation_id=x_correlation_id,
        traceparent=traceparent,
    ).parsed


async def asyncio_detailed(
    *,
    client: AuthenticatedClient | Client,
    limit: int | Unset = 50,
    before: str | Unset = UNSET,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> Response[ErrorEnvelope | UsageEventsResponse200]:
    """The tenant's recalls of the last 30 days, with what their readers did next, newest first.

     Requires: admin key role (ADR-0054 d2). No agent, writer, reader or dreamer key reads an event, on
    any transport. query_text is present only for a recall made while the tenant kept text. A row past
    the 30 days is never served, purged or not. system.unavailable with reason telemetry_off where the
    server records no recall. Since v0.5.7.

    Args:
        limit (int | Unset):  Default: 50.
        before (str | Unset):
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | UsageEventsResponse200]
    """

    kwargs = _get_kwargs(
        limit=limit,
        before=before,
        x_request_id=x_request_id,
        x_correlation_id=x_correlation_id,
        traceparent=traceparent,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    *,
    client: AuthenticatedClient | Client,
    limit: int | Unset = 50,
    before: str | Unset = UNSET,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> ErrorEnvelope | UsageEventsResponse200 | None:
    """The tenant's recalls of the last 30 days, with what their readers did next, newest first.

     Requires: admin key role (ADR-0054 d2). No agent, writer, reader or dreamer key reads an event, on
    any transport. query_text is present only for a recall made while the tenant kept text. A row past
    the 30 days is never served, purged or not. system.unavailable with reason telemetry_off where the
    server records no recall. Since v0.5.7.

    Args:
        limit (int | Unset):  Default: 50.
        before (str | Unset):
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | UsageEventsResponse200
    """

    return (
        await asyncio_detailed(
            client=client,
            limit=limit,
            before=before,
            x_request_id=x_request_id,
            x_correlation_id=x_correlation_id,
            traceparent=traceparent,
        )
    ).parsed
