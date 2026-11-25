from http import HTTPStatus
from typing import Any

import httpx

from ...client import AuthenticatedClient, Client
from ...models.error_envelope import ErrorEnvelope
from ...models.list_imports_response_200 import ListImportsResponse200
from ...models.list_imports_state import ListImportsState
from ...types import UNSET, Response, Unset


def _get_kwargs(
    *,
    state: ListImportsState | Unset = UNSET,
    limit: int | Unset = UNSET,
    cursor: str | Unset = UNSET,
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

    params["limit"] = limit

    params["cursor"] = cursor

    params = {k: v for k, v in params.items() if v is not UNSET and v is not None}

    _kwargs: dict[str, Any] = {
        "method": "get",
        "url": "/v2/imports",
        "params": params,
    }

    _kwargs["headers"] = headers
    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> ErrorEnvelope | ListImportsResponse200:
    if response.status_code == 200:
        response_200 = ListImportsResponse200.from_dict(response.json())

        return response_200

    if response.status_code == 400:
        response_400 = ErrorEnvelope.from_dict(response.json())

        return response_400

    response_default = ErrorEnvelope.from_dict(response.json())

    return response_default


def _build_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Response[ErrorEnvelope | ListImportsResponse200]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    *,
    client: AuthenticatedClient | Client,
    state: ListImportsState | Unset = UNSET,
    limit: int | Unset = UNSET,
    cursor: str | Unset = UNSET,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> Response[ErrorEnvelope | ListImportsResponse200]:
    """The imports a key ran, or as a person's web session every key of the person's, newest first.

     As a person's web session key, <user>-web, the imports of every key that names the person as its
    user, a tenant key linked to the person not included; as any other key, that key's own (ADR-0096
    d6). Newest first, at most 50 a page (limit from 1 to 50). state is running, done or failed; another
    value is request.validation with details.field state. Each item is the Import GET /v2/imports/{id}
    serves, with key, the importing key's id and name. Journals nothing. Since v0.8.5.

    Args:
        state (ListImportsState | Unset):
        limit (int | Unset):
        cursor (str | Unset):
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | ListImportsResponse200]
    """

    kwargs = _get_kwargs(
        state=state,
        limit=limit,
        cursor=cursor,
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
    state: ListImportsState | Unset = UNSET,
    limit: int | Unset = UNSET,
    cursor: str | Unset = UNSET,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> ErrorEnvelope | ListImportsResponse200 | None:
    """The imports a key ran, or as a person's web session every key of the person's, newest first.

     As a person's web session key, <user>-web, the imports of every key that names the person as its
    user, a tenant key linked to the person not included; as any other key, that key's own (ADR-0096
    d6). Newest first, at most 50 a page (limit from 1 to 50). state is running, done or failed; another
    value is request.validation with details.field state. Each item is the Import GET /v2/imports/{id}
    serves, with key, the importing key's id and name. Journals nothing. Since v0.8.5.

    Args:
        state (ListImportsState | Unset):
        limit (int | Unset):
        cursor (str | Unset):
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | ListImportsResponse200
    """

    return sync_detailed(
        client=client,
        state=state,
        limit=limit,
        cursor=cursor,
        x_request_id=x_request_id,
        x_correlation_id=x_correlation_id,
        traceparent=traceparent,
    ).parsed


async def asyncio_detailed(
    *,
    client: AuthenticatedClient | Client,
    state: ListImportsState | Unset = UNSET,
    limit: int | Unset = UNSET,
    cursor: str | Unset = UNSET,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> Response[ErrorEnvelope | ListImportsResponse200]:
    """The imports a key ran, or as a person's web session every key of the person's, newest first.

     As a person's web session key, <user>-web, the imports of every key that names the person as its
    user, a tenant key linked to the person not included; as any other key, that key's own (ADR-0096
    d6). Newest first, at most 50 a page (limit from 1 to 50). state is running, done or failed; another
    value is request.validation with details.field state. Each item is the Import GET /v2/imports/{id}
    serves, with key, the importing key's id and name. Journals nothing. Since v0.8.5.

    Args:
        state (ListImportsState | Unset):
        limit (int | Unset):
        cursor (str | Unset):
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | ListImportsResponse200]
    """

    kwargs = _get_kwargs(
        state=state,
        limit=limit,
        cursor=cursor,
        x_request_id=x_request_id,
        x_correlation_id=x_correlation_id,
        traceparent=traceparent,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    *,
    client: AuthenticatedClient | Client,
    state: ListImportsState | Unset = UNSET,
    limit: int | Unset = UNSET,
    cursor: str | Unset = UNSET,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> ErrorEnvelope | ListImportsResponse200 | None:
    """The imports a key ran, or as a person's web session every key of the person's, newest first.

     As a person's web session key, <user>-web, the imports of every key that names the person as its
    user, a tenant key linked to the person not included; as any other key, that key's own (ADR-0096
    d6). Newest first, at most 50 a page (limit from 1 to 50). state is running, done or failed; another
    value is request.validation with details.field state. Each item is the Import GET /v2/imports/{id}
    serves, with key, the importing key's id and name. Journals nothing. Since v0.8.5.

    Args:
        state (ListImportsState | Unset):
        limit (int | Unset):
        cursor (str | Unset):
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | ListImportsResponse200
    """

    return (
        await asyncio_detailed(
            client=client,
            state=state,
            limit=limit,
            cursor=cursor,
            x_request_id=x_request_id,
            x_correlation_id=x_correlation_id,
            traceparent=traceparent,
        )
    ).parsed
