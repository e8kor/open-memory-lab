from http import HTTPStatus
from typing import Any

import httpx

from ...client import AuthenticatedClient, Client
from ...models.error_envelope import ErrorEnvelope
from ...models.list_keys_response_200 import ListKeysResponse200
from ...types import UNSET, Response, Unset


def _get_kwargs(
    *,
    user: str | Unset = UNSET,
    limit: int | Unset = 50,
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

    params["user"] = user

    params["limit"] = limit

    params["cursor"] = cursor

    params = {k: v for k, v in params.items() if v is not UNSET and v is not None}

    _kwargs: dict[str, Any] = {
        "method": "get",
        "url": "/v2/keys",
        "params": params,
    }

    _kwargs["headers"] = headers
    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> ErrorEnvelope | ListKeysResponse200:
    if response.status_code == 200:
        response_200 = ListKeysResponse200.from_dict(response.json())

        return response_200

    if response.status_code == 403:
        response_403 = ErrorEnvelope.from_dict(response.json())

        return response_403

    if response.status_code == 404:
        response_404 = ErrorEnvelope.from_dict(response.json())

        return response_404

    response_default = ErrorEnvelope.from_dict(response.json())

    return response_default


def _build_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Response[ErrorEnvelope | ListKeysResponse200]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    *,
    client: AuthenticatedClient | Client,
    user: str | Unset = UNSET,
    limit: int | Unset = 50,
    cursor: str | Unset = UNSET,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> Response[ErrorEnvelope | ListKeysResponse200]:
    """A person's own keys to their web session key; the keys naming a user to the admin key role.

     Requires: a person's web session key (<user>-web, through its capacity credential), which lists
    every key naming its user, session keys among them; or the admin key role with user, which lists the
    keys naming that user, session keys aside (ADR-0045 d9, d13; ADR-0070 d5). Any other key answers
    access.verb_ceiling with details.reason session_key_only, so an agent's key never lists its person's
    keys; the admin key role without user answers request.validation naming user; a person's key naming
    another user answers access.not_found. In name order; next_cursor is opaque. Each key's grants carry
    the workarea's path; cut_off is a denial on universe. Journals nothing. Since v0.6.4.

    Args:
        user (str | Unset):
        limit (int | Unset):  Default: 50.
        cursor (str | Unset):
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | ListKeysResponse200]
    """

    kwargs = _get_kwargs(
        user=user,
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
    user: str | Unset = UNSET,
    limit: int | Unset = 50,
    cursor: str | Unset = UNSET,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> ErrorEnvelope | ListKeysResponse200 | None:
    """A person's own keys to their web session key; the keys naming a user to the admin key role.

     Requires: a person's web session key (<user>-web, through its capacity credential), which lists
    every key naming its user, session keys among them; or the admin key role with user, which lists the
    keys naming that user, session keys aside (ADR-0045 d9, d13; ADR-0070 d5). Any other key answers
    access.verb_ceiling with details.reason session_key_only, so an agent's key never lists its person's
    keys; the admin key role without user answers request.validation naming user; a person's key naming
    another user answers access.not_found. In name order; next_cursor is opaque. Each key's grants carry
    the workarea's path; cut_off is a denial on universe. Journals nothing. Since v0.6.4.

    Args:
        user (str | Unset):
        limit (int | Unset):  Default: 50.
        cursor (str | Unset):
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | ListKeysResponse200
    """

    return sync_detailed(
        client=client,
        user=user,
        limit=limit,
        cursor=cursor,
        x_request_id=x_request_id,
        x_correlation_id=x_correlation_id,
        traceparent=traceparent,
    ).parsed


async def asyncio_detailed(
    *,
    client: AuthenticatedClient | Client,
    user: str | Unset = UNSET,
    limit: int | Unset = 50,
    cursor: str | Unset = UNSET,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> Response[ErrorEnvelope | ListKeysResponse200]:
    """A person's own keys to their web session key; the keys naming a user to the admin key role.

     Requires: a person's web session key (<user>-web, through its capacity credential), which lists
    every key naming its user, session keys among them; or the admin key role with user, which lists the
    keys naming that user, session keys aside (ADR-0045 d9, d13; ADR-0070 d5). Any other key answers
    access.verb_ceiling with details.reason session_key_only, so an agent's key never lists its person's
    keys; the admin key role without user answers request.validation naming user; a person's key naming
    another user answers access.not_found. In name order; next_cursor is opaque. Each key's grants carry
    the workarea's path; cut_off is a denial on universe. Journals nothing. Since v0.6.4.

    Args:
        user (str | Unset):
        limit (int | Unset):  Default: 50.
        cursor (str | Unset):
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | ListKeysResponse200]
    """

    kwargs = _get_kwargs(
        user=user,
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
    user: str | Unset = UNSET,
    limit: int | Unset = 50,
    cursor: str | Unset = UNSET,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> ErrorEnvelope | ListKeysResponse200 | None:
    """A person's own keys to their web session key; the keys naming a user to the admin key role.

     Requires: a person's web session key (<user>-web, through its capacity credential), which lists
    every key naming its user, session keys among them; or the admin key role with user, which lists the
    keys naming that user, session keys aside (ADR-0045 d9, d13; ADR-0070 d5). Any other key answers
    access.verb_ceiling with details.reason session_key_only, so an agent's key never lists its person's
    keys; the admin key role without user answers request.validation naming user; a person's key naming
    another user answers access.not_found. In name order; next_cursor is opaque. Each key's grants carry
    the workarea's path; cut_off is a denial on universe. Journals nothing. Since v0.6.4.

    Args:
        user (str | Unset):
        limit (int | Unset):  Default: 50.
        cursor (str | Unset):
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | ListKeysResponse200
    """

    return (
        await asyncio_detailed(
            client=client,
            user=user,
            limit=limit,
            cursor=cursor,
            x_request_id=x_request_id,
            x_correlation_id=x_correlation_id,
            traceparent=traceparent,
        )
    ).parsed
