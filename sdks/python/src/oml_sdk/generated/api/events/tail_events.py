from http import HTTPStatus
from typing import Any

import httpx

from ...client import AuthenticatedClient, Client
from ...models.error_envelope import ErrorEnvelope
from ...models.events_page import EventsPage
from ...types import UNSET, Response, Unset


def _get_kwargs(
    *,
    after_seq: int | Unset = 0,
    workarea_id: str | Unset = UNSET,
    limit: int | Unset = 50,
) -> dict[str, Any]:

    params: dict[str, Any] = {}

    params["after_seq"] = after_seq

    params["workarea_id"] = workarea_id

    params["limit"] = limit

    params = {k: v for k, v in params.items() if v is not UNSET and v is not None}

    _kwargs: dict[str, Any] = {
        "method": "get",
        "url": "/v2/events",
        "params": params,
    }

    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> ErrorEnvelope | EventsPage:
    if response.status_code == 200:
        response_200 = EventsPage.from_dict(response.json())

        return response_200

    if response.status_code == 429:
        response_429 = ErrorEnvelope.from_dict(response.json())

        return response_429

    response_default = ErrorEnvelope.from_dict(response.json())

    return response_default


def _build_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Response[ErrorEnvelope | EventsPage]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    *,
    client: AuthenticatedClient | Client,
    after_seq: int | Unset = 0,
    workarea_id: str | Unset = UNSET,
    limit: int | Unset = 50,
) -> Response[ErrorEnvelope | EventsPage]:
    """The per-tenant journal as digest-only envelopes; out-of-scope events omitted (a feed, not a
    verification surface).

     Requires: reader effective access on workarea_id (access.not_found otherwise); omitted, the key's
    default workarea, and request.workarea_unresolved for a key with none. Which events a key sees, what
    a served event names, and how next_after_seq is computed, are declared in docs/api/http-api.md,
    Events tail (ADR-0025 d15): an event served to a key outside its actor's own key set carries
    principal, api_key_id and every key-naming payload field as null, as the example shows for a reader
    seeing another key's ratification. One call scans at most limit raw events from after_seq + 1;
    after_seq greater than head_seq answers 400 request.validation. Full-chain verification is served by
    the chain export.

    Args:
        after_seq (int | Unset):  Default: 0.
        workarea_id (str | Unset):
        limit (int | Unset):  Default: 50.

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | EventsPage]
    """

    kwargs = _get_kwargs(
        after_seq=after_seq,
        workarea_id=workarea_id,
        limit=limit,
    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    *,
    client: AuthenticatedClient | Client,
    after_seq: int | Unset = 0,
    workarea_id: str | Unset = UNSET,
    limit: int | Unset = 50,
) -> ErrorEnvelope | EventsPage | None:
    """The per-tenant journal as digest-only envelopes; out-of-scope events omitted (a feed, not a
    verification surface).

     Requires: reader effective access on workarea_id (access.not_found otherwise); omitted, the key's
    default workarea, and request.workarea_unresolved for a key with none. Which events a key sees, what
    a served event names, and how next_after_seq is computed, are declared in docs/api/http-api.md,
    Events tail (ADR-0025 d15): an event served to a key outside its actor's own key set carries
    principal, api_key_id and every key-naming payload field as null, as the example shows for a reader
    seeing another key's ratification. One call scans at most limit raw events from after_seq + 1;
    after_seq greater than head_seq answers 400 request.validation. Full-chain verification is served by
    the chain export.

    Args:
        after_seq (int | Unset):  Default: 0.
        workarea_id (str | Unset):
        limit (int | Unset):  Default: 50.

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | EventsPage
    """

    return sync_detailed(
        client=client,
        after_seq=after_seq,
        workarea_id=workarea_id,
        limit=limit,
    ).parsed


async def asyncio_detailed(
    *,
    client: AuthenticatedClient | Client,
    after_seq: int | Unset = 0,
    workarea_id: str | Unset = UNSET,
    limit: int | Unset = 50,
) -> Response[ErrorEnvelope | EventsPage]:
    """The per-tenant journal as digest-only envelopes; out-of-scope events omitted (a feed, not a
    verification surface).

     Requires: reader effective access on workarea_id (access.not_found otherwise); omitted, the key's
    default workarea, and request.workarea_unresolved for a key with none. Which events a key sees, what
    a served event names, and how next_after_seq is computed, are declared in docs/api/http-api.md,
    Events tail (ADR-0025 d15): an event served to a key outside its actor's own key set carries
    principal, api_key_id and every key-naming payload field as null, as the example shows for a reader
    seeing another key's ratification. One call scans at most limit raw events from after_seq + 1;
    after_seq greater than head_seq answers 400 request.validation. Full-chain verification is served by
    the chain export.

    Args:
        after_seq (int | Unset):  Default: 0.
        workarea_id (str | Unset):
        limit (int | Unset):  Default: 50.

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | EventsPage]
    """

    kwargs = _get_kwargs(
        after_seq=after_seq,
        workarea_id=workarea_id,
        limit=limit,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    *,
    client: AuthenticatedClient | Client,
    after_seq: int | Unset = 0,
    workarea_id: str | Unset = UNSET,
    limit: int | Unset = 50,
) -> ErrorEnvelope | EventsPage | None:
    """The per-tenant journal as digest-only envelopes; out-of-scope events omitted (a feed, not a
    verification surface).

     Requires: reader effective access on workarea_id (access.not_found otherwise); omitted, the key's
    default workarea, and request.workarea_unresolved for a key with none. Which events a key sees, what
    a served event names, and how next_after_seq is computed, are declared in docs/api/http-api.md,
    Events tail (ADR-0025 d15): an event served to a key outside its actor's own key set carries
    principal, api_key_id and every key-naming payload field as null, as the example shows for a reader
    seeing another key's ratification. One call scans at most limit raw events from after_seq + 1;
    after_seq greater than head_seq answers 400 request.validation. Full-chain verification is served by
    the chain export.

    Args:
        after_seq (int | Unset):  Default: 0.
        workarea_id (str | Unset):
        limit (int | Unset):  Default: 50.

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | EventsPage
    """

    return (
        await asyncio_detailed(
            client=client,
            after_seq=after_seq,
            workarea_id=workarea_id,
            limit=limit,
        )
    ).parsed
