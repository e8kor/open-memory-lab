from http import HTTPStatus
from typing import Any

import httpx

from ...client import AuthenticatedClient, Client
from ...models.error_envelope import ErrorEnvelope
from ...models.whoami import Whoami
from ...types import UNSET, Response, Unset


def _get_kwargs(
    *,
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

    _kwargs: dict[str, Any] = {
        "method": "get",
        "url": "/v2/whoami",
    }

    _kwargs["headers"] = headers
    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> ErrorEnvelope | Whoami:
    if response.status_code == 200:
        response_200 = Whoami.from_dict(response.json())

        return response_200

    if response.status_code == 401:
        response_401 = ErrorEnvelope.from_dict(response.json())

        return response_401

    response_default = ErrorEnvelope.from_dict(response.json())

    return response_default


def _build_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Response[ErrorEnvelope | Whoami]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    *,
    client: AuthenticatedClient | Client,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> Response[ErrorEnvelope | Whoami]:
    """The calling key's own record — role, name, tenant, default workarea, crew and grants. Never its
    denials or deny masks.

     Requires: any live key; it reads only its own record. Each reference is {id, name} and the default
    workarea carries its derived path. The key's denials and its grants' per-grant deny masks, which
    name subtrees it may not read, are omitted; the read journals nothing, since it reads no claim. A
    revoked or unknown key answers auth.invalid as on every route (docs/api/http-api.md, Whoami;
    ADR-0039 d15).

    Args:
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | Whoami]
    """

    kwargs = _get_kwargs(
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
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> ErrorEnvelope | Whoami | None:
    """The calling key's own record — role, name, tenant, default workarea, crew and grants. Never its
    denials or deny masks.

     Requires: any live key; it reads only its own record. Each reference is {id, name} and the default
    workarea carries its derived path. The key's denials and its grants' per-grant deny masks, which
    name subtrees it may not read, are omitted; the read journals nothing, since it reads no claim. A
    revoked or unknown key answers auth.invalid as on every route (docs/api/http-api.md, Whoami;
    ADR-0039 d15).

    Args:
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | Whoami
    """

    return sync_detailed(
        client=client,
        x_request_id=x_request_id,
        x_correlation_id=x_correlation_id,
        traceparent=traceparent,
    ).parsed


async def asyncio_detailed(
    *,
    client: AuthenticatedClient | Client,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> Response[ErrorEnvelope | Whoami]:
    """The calling key's own record — role, name, tenant, default workarea, crew and grants. Never its
    denials or deny masks.

     Requires: any live key; it reads only its own record. Each reference is {id, name} and the default
    workarea carries its derived path. The key's denials and its grants' per-grant deny masks, which
    name subtrees it may not read, are omitted; the read journals nothing, since it reads no claim. A
    revoked or unknown key answers auth.invalid as on every route (docs/api/http-api.md, Whoami;
    ADR-0039 d15).

    Args:
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | Whoami]
    """

    kwargs = _get_kwargs(
        x_request_id=x_request_id,
        x_correlation_id=x_correlation_id,
        traceparent=traceparent,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    *,
    client: AuthenticatedClient | Client,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> ErrorEnvelope | Whoami | None:
    """The calling key's own record — role, name, tenant, default workarea, crew and grants. Never its
    denials or deny masks.

     Requires: any live key; it reads only its own record. Each reference is {id, name} and the default
    workarea carries its derived path. The key's denials and its grants' per-grant deny masks, which
    name subtrees it may not read, are omitted; the read journals nothing, since it reads no claim. A
    revoked or unknown key answers auth.invalid as on every route (docs/api/http-api.md, Whoami;
    ADR-0039 d15).

    Args:
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | Whoami
    """

    return (
        await asyncio_detailed(
            client=client,
            x_request_id=x_request_id,
            x_correlation_id=x_correlation_id,
            traceparent=traceparent,
        )
    ).parsed
