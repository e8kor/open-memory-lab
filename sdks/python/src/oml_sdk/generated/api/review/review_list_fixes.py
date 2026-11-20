from http import HTTPStatus
from typing import Any

import httpx

from ...client import AuthenticatedClient, Client
from ...models.error_envelope import ErrorEnvelope
from ...models.review_list_fixes_response_200 import ReviewListFixesResponse200
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
        "url": "/v2/review/fixes",
    }

    _kwargs["headers"] = headers
    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> ErrorEnvelope | ReviewListFixesResponse200:
    if response.status_code == 200:
        response_200 = ReviewListFixesResponse200.from_dict(response.json())

        return response_200

    if response.status_code == 403:
        response_403 = ErrorEnvelope.from_dict(response.json())

        return response_403

    response_default = ErrorEnvelope.from_dict(response.json())

    return response_default


def _build_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Response[ErrorEnvelope | ReviewListFixesResponse200]:
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
) -> Response[ErrorEnvelope | ReviewListFixesResponse200]:
    """The unconfirmed admin fixes on workspaces where this person's reviewer key holds reviewer, each with
    the acts this key may take.

     Requires: a person's reviewer key: reviewer role, used by a person, no crew and no profile (ADR-0089
    d15). Never names the admin key, principal or user. acts is [confirm, revert], [confirm] for an
    erasure, or [] when this key acts for the admin's own person and the fix's target lies outside that
    person's own root, sits in a protected or frozen workspace, or is a member of a release (ADR-0089,
    amendment of 2026-11-18). Journals nothing. Since v0.8.4.

    Args:
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | ReviewListFixesResponse200]
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
) -> ErrorEnvelope | ReviewListFixesResponse200 | None:
    """The unconfirmed admin fixes on workspaces where this person's reviewer key holds reviewer, each with
    the acts this key may take.

     Requires: a person's reviewer key: reviewer role, used by a person, no crew and no profile (ADR-0089
    d15). Never names the admin key, principal or user. acts is [confirm, revert], [confirm] for an
    erasure, or [] when this key acts for the admin's own person and the fix's target lies outside that
    person's own root, sits in a protected or frozen workspace, or is a member of a release (ADR-0089,
    amendment of 2026-11-18). Journals nothing. Since v0.8.4.

    Args:
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | ReviewListFixesResponse200
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
) -> Response[ErrorEnvelope | ReviewListFixesResponse200]:
    """The unconfirmed admin fixes on workspaces where this person's reviewer key holds reviewer, each with
    the acts this key may take.

     Requires: a person's reviewer key: reviewer role, used by a person, no crew and no profile (ADR-0089
    d15). Never names the admin key, principal or user. acts is [confirm, revert], [confirm] for an
    erasure, or [] when this key acts for the admin's own person and the fix's target lies outside that
    person's own root, sits in a protected or frozen workspace, or is a member of a release (ADR-0089,
    amendment of 2026-11-18). Journals nothing. Since v0.8.4.

    Args:
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | ReviewListFixesResponse200]
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
) -> ErrorEnvelope | ReviewListFixesResponse200 | None:
    """The unconfirmed admin fixes on workspaces where this person's reviewer key holds reviewer, each with
    the acts this key may take.

     Requires: a person's reviewer key: reviewer role, used by a person, no crew and no profile (ADR-0089
    d15). Never names the admin key, principal or user. acts is [confirm, revert], [confirm] for an
    erasure, or [] when this key acts for the admin's own person and the fix's target lies outside that
    person's own root, sits in a protected or frozen workspace, or is a member of a release (ADR-0089,
    amendment of 2026-11-18). Journals nothing. Since v0.8.4.

    Args:
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | ReviewListFixesResponse200
    """

    return (
        await asyncio_detailed(
            client=client,
            x_request_id=x_request_id,
            x_correlation_id=x_correlation_id,
            traceparent=traceparent,
        )
    ).parsed
