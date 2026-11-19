from http import HTTPStatus
from typing import Any

import httpx

from ...client import AuthenticatedClient, Client
from ...models.admin_list_fixes_response_200 import AdminListFixesResponse200
from ...models.admin_list_fixes_state import AdminListFixesState
from ...models.error_envelope import ErrorEnvelope
from ...types import UNSET, Response, Unset


def _get_kwargs(
    *,
    state: AdminListFixesState | Unset = AdminListFixesState.UNCONFIRMED,
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
        "url": "/v2/admin/fixes",
        "params": params,
    }

    _kwargs["headers"] = headers
    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> AdminListFixesResponse200 | ErrorEnvelope:
    if response.status_code == 200:
        response_200 = AdminListFixesResponse200.from_dict(response.json())

        return response_200

    if response.status_code == 403:
        response_403 = ErrorEnvelope.from_dict(response.json())

        return response_403

    response_default = ErrorEnvelope.from_dict(response.json())

    return response_default


def _build_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Response[AdminListFixesResponse200 | ErrorEnvelope]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    *,
    client: AuthenticatedClient | Client,
    state: AdminListFixesState | Unset = AdminListFixesState.UNCONFIRMED,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> Response[AdminListFixesResponse200 | ErrorEnvelope]:
    """The tenant's admin fixes, the unconfirmed ones by default.

     Requires: admin key role (ADR-0089 d15). Oldest first. Journals nothing. Since v0.8.4.

    Args:
        state (AdminListFixesState | Unset):  Default: AdminListFixesState.UNCONFIRMED.
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[AdminListFixesResponse200 | ErrorEnvelope]
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
    state: AdminListFixesState | Unset = AdminListFixesState.UNCONFIRMED,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> AdminListFixesResponse200 | ErrorEnvelope | None:
    """The tenant's admin fixes, the unconfirmed ones by default.

     Requires: admin key role (ADR-0089 d15). Oldest first. Journals nothing. Since v0.8.4.

    Args:
        state (AdminListFixesState | Unset):  Default: AdminListFixesState.UNCONFIRMED.
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        AdminListFixesResponse200 | ErrorEnvelope
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
    state: AdminListFixesState | Unset = AdminListFixesState.UNCONFIRMED,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> Response[AdminListFixesResponse200 | ErrorEnvelope]:
    """The tenant's admin fixes, the unconfirmed ones by default.

     Requires: admin key role (ADR-0089 d15). Oldest first. Journals nothing. Since v0.8.4.

    Args:
        state (AdminListFixesState | Unset):  Default: AdminListFixesState.UNCONFIRMED.
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[AdminListFixesResponse200 | ErrorEnvelope]
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
    state: AdminListFixesState | Unset = AdminListFixesState.UNCONFIRMED,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> AdminListFixesResponse200 | ErrorEnvelope | None:
    """The tenant's admin fixes, the unconfirmed ones by default.

     Requires: admin key role (ADR-0089 d15). Oldest first. Journals nothing. Since v0.8.4.

    Args:
        state (AdminListFixesState | Unset):  Default: AdminListFixesState.UNCONFIRMED.
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        AdminListFixesResponse200 | ErrorEnvelope
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
