from http import HTTPStatus
from typing import Any
from urllib.parse import quote

import httpx

from ...client import AuthenticatedClient, Client
from ...models.account_profile_request import AccountProfileRequest
from ...models.account_usage import AccountUsage
from ...models.error_envelope import ErrorEnvelope
from ...types import UNSET, Response, Unset


def _get_kwargs(
    account: str,
    *,
    body: AccountProfileRequest,
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
        "method": "put",
        "url": "/v2/admin/accounts/{account}/profile".format(
            account=quote(str(account), safe=""),
        ),
    }

    _kwargs["json"] = body.to_dict()

    headers["Content-Type"] = "application/json"

    _kwargs["headers"] = headers
    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> AccountUsage | ErrorEnvelope:
    if response.status_code == 200:
        response_200 = AccountUsage.from_dict(response.json())

        return response_200

    if response.status_code == 400:
        response_400 = ErrorEnvelope.from_dict(response.json())

        return response_400

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
) -> Response[AccountUsage | ErrorEnvelope]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    account: str,
    *,
    client: AuthenticatedClient | Client,
    body: AccountProfileRequest,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> Response[AccountUsage | ErrorEnvelope]:
    """Set an account's quota profile and its overrides, replacing the ones it had.

     Requires: admin key role (ADR-0071 d5, d6). profile names a profile the configuration declares, or
    request.validation with details.field profile; overrides maps a quota to its limit, each from 0 to
    that quota's maximum (1,000,000, or 1,000,000,000,000 for text_bytes), and an unknown quota is
    request.validation with details.field overrides. Omitted overrides clear the account's. Journalled
    as account.profile_set; the new limits hold from the next write. Answers the account's usage under
    them. Idempotent by its replace semantics; no Idempotency-Key. Proposed.

    Args:
        account (str):
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        body (AccountProfileRequest):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[AccountUsage | ErrorEnvelope]
    """

    kwargs = _get_kwargs(
        account=account,
        body=body,
        x_request_id=x_request_id,
        x_correlation_id=x_correlation_id,
        traceparent=traceparent,
    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    account: str,
    *,
    client: AuthenticatedClient | Client,
    body: AccountProfileRequest,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> AccountUsage | ErrorEnvelope | None:
    """Set an account's quota profile and its overrides, replacing the ones it had.

     Requires: admin key role (ADR-0071 d5, d6). profile names a profile the configuration declares, or
    request.validation with details.field profile; overrides maps a quota to its limit, each from 0 to
    that quota's maximum (1,000,000, or 1,000,000,000,000 for text_bytes), and an unknown quota is
    request.validation with details.field overrides. Omitted overrides clear the account's. Journalled
    as account.profile_set; the new limits hold from the next write. Answers the account's usage under
    them. Idempotent by its replace semantics; no Idempotency-Key. Proposed.

    Args:
        account (str):
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        body (AccountProfileRequest):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        AccountUsage | ErrorEnvelope
    """

    return sync_detailed(
        account=account,
        client=client,
        body=body,
        x_request_id=x_request_id,
        x_correlation_id=x_correlation_id,
        traceparent=traceparent,
    ).parsed


async def asyncio_detailed(
    account: str,
    *,
    client: AuthenticatedClient | Client,
    body: AccountProfileRequest,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> Response[AccountUsage | ErrorEnvelope]:
    """Set an account's quota profile and its overrides, replacing the ones it had.

     Requires: admin key role (ADR-0071 d5, d6). profile names a profile the configuration declares, or
    request.validation with details.field profile; overrides maps a quota to its limit, each from 0 to
    that quota's maximum (1,000,000, or 1,000,000,000,000 for text_bytes), and an unknown quota is
    request.validation with details.field overrides. Omitted overrides clear the account's. Journalled
    as account.profile_set; the new limits hold from the next write. Answers the account's usage under
    them. Idempotent by its replace semantics; no Idempotency-Key. Proposed.

    Args:
        account (str):
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        body (AccountProfileRequest):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[AccountUsage | ErrorEnvelope]
    """

    kwargs = _get_kwargs(
        account=account,
        body=body,
        x_request_id=x_request_id,
        x_correlation_id=x_correlation_id,
        traceparent=traceparent,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    account: str,
    *,
    client: AuthenticatedClient | Client,
    body: AccountProfileRequest,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> AccountUsage | ErrorEnvelope | None:
    """Set an account's quota profile and its overrides, replacing the ones it had.

     Requires: admin key role (ADR-0071 d5, d6). profile names a profile the configuration declares, or
    request.validation with details.field profile; overrides maps a quota to its limit, each from 0 to
    that quota's maximum (1,000,000, or 1,000,000,000,000 for text_bytes), and an unknown quota is
    request.validation with details.field overrides. Omitted overrides clear the account's. Journalled
    as account.profile_set; the new limits hold from the next write. Answers the account's usage under
    them. Idempotent by its replace semantics; no Idempotency-Key. Proposed.

    Args:
        account (str):
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        body (AccountProfileRequest):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        AccountUsage | ErrorEnvelope
    """

    return (
        await asyncio_detailed(
            account=account,
            client=client,
            body=body,
            x_request_id=x_request_id,
            x_correlation_id=x_correlation_id,
            traceparent=traceparent,
        )
    ).parsed
