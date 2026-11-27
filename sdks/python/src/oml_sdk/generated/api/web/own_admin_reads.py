from http import HTTPStatus
from typing import Any

import httpx

from ...client import AuthenticatedClient, Client
from ...models.error_envelope import ErrorEnvelope
from ...models.own_admin_reads_response_200 import OwnAdminReadsResponse200
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
        "url": "/v2/web/admin-reads",
    }

    _kwargs["headers"] = headers
    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> ErrorEnvelope | OwnAdminReadsResponse200:
    if response.status_code == 200:
        response_200 = OwnAdminReadsResponse200.from_dict(response.json())

        return response_200

    if response.status_code == 401:
        response_401 = ErrorEnvelope.from_dict(response.json())

        return response_401

    if response.status_code == 404:
        response_404 = ErrorEnvelope.from_dict(response.json())

        return response_404

    response_default = ErrorEnvelope.from_dict(response.json())

    return response_default


def _build_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Response[ErrorEnvelope | OwnAdminReadsResponse200]:
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
) -> Response[ErrorEnvelope | OwnAdminReadsResponse200]:
    """The count of admin reads of the calling person's own root, by month.

     Requires: a key that counts as a person, the person's session keys among them; one that counts as no
    person answers access.not_found. Each admin-read page that served a row of the person's own root
    counts once, for the month of the read (UTC). Reads through keys or connect codes a second admin
    confirmed in the root are the root's own reads and are not counted (ADR-0097 d6, d16). Journals
    nothing. Since v0.8.6.

    Args:
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | OwnAdminReadsResponse200]
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
) -> ErrorEnvelope | OwnAdminReadsResponse200 | None:
    """The count of admin reads of the calling person's own root, by month.

     Requires: a key that counts as a person, the person's session keys among them; one that counts as no
    person answers access.not_found. Each admin-read page that served a row of the person's own root
    counts once, for the month of the read (UTC). Reads through keys or connect codes a second admin
    confirmed in the root are the root's own reads and are not counted (ADR-0097 d6, d16). Journals
    nothing. Since v0.8.6.

    Args:
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | OwnAdminReadsResponse200
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
) -> Response[ErrorEnvelope | OwnAdminReadsResponse200]:
    """The count of admin reads of the calling person's own root, by month.

     Requires: a key that counts as a person, the person's session keys among them; one that counts as no
    person answers access.not_found. Each admin-read page that served a row of the person's own root
    counts once, for the month of the read (UTC). Reads through keys or connect codes a second admin
    confirmed in the root are the root's own reads and are not counted (ADR-0097 d6, d16). Journals
    nothing. Since v0.8.6.

    Args:
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | OwnAdminReadsResponse200]
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
) -> ErrorEnvelope | OwnAdminReadsResponse200 | None:
    """The count of admin reads of the calling person's own root, by month.

     Requires: a key that counts as a person, the person's session keys among them; one that counts as no
    person answers access.not_found. Each admin-read page that served a row of the person's own root
    counts once, for the month of the read (UTC). Reads through keys or connect codes a second admin
    confirmed in the root are the root's own reads and are not counted (ADR-0097 d6, d16). Journals
    nothing. Since v0.8.6.

    Args:
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | OwnAdminReadsResponse200
    """

    return (
        await asyncio_detailed(
            client=client,
            x_request_id=x_request_id,
            x_correlation_id=x_correlation_id,
            traceparent=traceparent,
        )
    ).parsed
