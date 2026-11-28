from http import HTTPStatus
from typing import Any

import httpx

from ...client import AuthenticatedClient, Client
from ...models.error_envelope import ErrorEnvelope
from ...models.list_admin_acts_response_200 import ListAdminActsResponse200
from ...models.list_admin_acts_state import ListAdminActsState
from ...types import UNSET, Response, Unset


def _get_kwargs(
    *,
    state: ListAdminActsState | Unset = ListAdminActsState.PENDING_CONFIRMATION,
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

    json_state: str | Unset = UNSET
    if not isinstance(state, Unset):
        json_state = state.value

    params["state"] = json_state

    params = {k: v for k, v in params.items() if v is not UNSET and v is not None}

    _kwargs: dict[str, Any] = {
        "method": "get",
        "url": "/v2/admin/acts",
        "params": params,
    }

    _kwargs["headers"] = headers
    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> ErrorEnvelope | ListAdminActsResponse200:
    if response.status_code == 200:
        response_200 = ListAdminActsResponse200.from_dict(response.json())

        return response_200

    if response.status_code == 403:
        response_403 = ErrorEnvelope.from_dict(response.json())

        return response_403

    response_default = ErrorEnvelope.from_dict(response.json())

    return response_default


def _build_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Response[ErrorEnvelope | ListAdminActsResponse200]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    *,
    client: AuthenticatedClient | Client,
    state: ListAdminActsState | Unset = ListAdminActsState.PENDING_CONFIRMATION,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> Response[ErrorEnvelope | ListAdminActsResponse200]:
    """The tenant's admin acts, those waiting on a second admin person by default.

     Requires: admin key role. Every admin act outside content review is one record (ADR-0097 d1, d2):
    its class, fixed by the act (narrowing, erase, widening, consent, against_admin, neutral), its
    state, the person the acting key counted as, its reason and, for a widening act, what its
    confirmation applies. state is all, or one state; omitted, pending_confirmation. A pending act seven
    days old reads expired, and is final (ADR-0097 d23). Oldest first. Journals nothing. Since v0.8.6.

    Args:
        state (ListAdminActsState | Unset):  Default: ListAdminActsState.PENDING_CONFIRMATION.
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | ListAdminActsResponse200]
    """

    kwargs = _get_kwargs(
        state=state,
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
    state: ListAdminActsState | Unset = ListAdminActsState.PENDING_CONFIRMATION,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> ErrorEnvelope | ListAdminActsResponse200 | None:
    """The tenant's admin acts, those waiting on a second admin person by default.

     Requires: admin key role. Every admin act outside content review is one record (ADR-0097 d1, d2):
    its class, fixed by the act (narrowing, erase, widening, consent, against_admin, neutral), its
    state, the person the acting key counted as, its reason and, for a widening act, what its
    confirmation applies. state is all, or one state; omitted, pending_confirmation. A pending act seven
    days old reads expired, and is final (ADR-0097 d23). Oldest first. Journals nothing. Since v0.8.6.

    Args:
        state (ListAdminActsState | Unset):  Default: ListAdminActsState.PENDING_CONFIRMATION.
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | ListAdminActsResponse200
    """

    return sync_detailed(
        client=client,
        state=state,
        x_request_id=x_request_id,
        x_correlation_id=x_correlation_id,
        traceparent=traceparent,
    ).parsed


async def asyncio_detailed(
    *,
    client: AuthenticatedClient | Client,
    state: ListAdminActsState | Unset = ListAdminActsState.PENDING_CONFIRMATION,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> Response[ErrorEnvelope | ListAdminActsResponse200]:
    """The tenant's admin acts, those waiting on a second admin person by default.

     Requires: admin key role. Every admin act outside content review is one record (ADR-0097 d1, d2):
    its class, fixed by the act (narrowing, erase, widening, consent, against_admin, neutral), its
    state, the person the acting key counted as, its reason and, for a widening act, what its
    confirmation applies. state is all, or one state; omitted, pending_confirmation. A pending act seven
    days old reads expired, and is final (ADR-0097 d23). Oldest first. Journals nothing. Since v0.8.6.

    Args:
        state (ListAdminActsState | Unset):  Default: ListAdminActsState.PENDING_CONFIRMATION.
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | ListAdminActsResponse200]
    """

    kwargs = _get_kwargs(
        state=state,
        x_request_id=x_request_id,
        x_correlation_id=x_correlation_id,
        traceparent=traceparent,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    *,
    client: AuthenticatedClient | Client,
    state: ListAdminActsState | Unset = ListAdminActsState.PENDING_CONFIRMATION,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> ErrorEnvelope | ListAdminActsResponse200 | None:
    """The tenant's admin acts, those waiting on a second admin person by default.

     Requires: admin key role. Every admin act outside content review is one record (ADR-0097 d1, d2):
    its class, fixed by the act (narrowing, erase, widening, consent, against_admin, neutral), its
    state, the person the acting key counted as, its reason and, for a widening act, what its
    confirmation applies. state is all, or one state; omitted, pending_confirmation. A pending act seven
    days old reads expired, and is final (ADR-0097 d23). Oldest first. Journals nothing. Since v0.8.6.

    Args:
        state (ListAdminActsState | Unset):  Default: ListAdminActsState.PENDING_CONFIRMATION.
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | ListAdminActsResponse200
    """

    return (
        await asyncio_detailed(
            client=client,
            state=state,
            x_request_id=x_request_id,
            x_correlation_id=x_correlation_id,
            traceparent=traceparent,
        )
    ).parsed
