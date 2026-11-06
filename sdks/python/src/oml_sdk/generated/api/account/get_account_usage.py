from http import HTTPStatus
from typing import Any

import httpx

from ...client import AuthenticatedClient, Client
from ...models.account_usage import AccountUsage
from ...models.error_envelope import ErrorEnvelope
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
        "url": "/v1/account/usage",
    }

    _kwargs["headers"] = headers
    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> AccountUsage | ErrorEnvelope:
    if response.status_code == 200:
        response_200 = AccountUsage.from_dict(response.json())

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
) -> Response[AccountUsage | ErrorEnvelope]:
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
) -> Response[AccountUsage | ErrorEnvelope]:
    """The calling key's own account's meters against its limits this UTC month.

     Requires: any key of an account, a user root's key (ADR-0071 d1, d6). The account is the key's own
    and no parameter names another, so no request reads another account's usage; a tenant key, which
    belongs to no account, answers access.not_found. Each meter carries its count, its limit (null where
    none holds), the whole percentage and its state: warning at 80% of the limit, reached at it
    (ADR-0071 d5). Counts only, never content or query text (d7). Reading never blocks: no quota refuses
    this or any other read. Proposed.

    Args:
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[AccountUsage | ErrorEnvelope]
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
) -> AccountUsage | ErrorEnvelope | None:
    """The calling key's own account's meters against its limits this UTC month.

     Requires: any key of an account, a user root's key (ADR-0071 d1, d6). The account is the key's own
    and no parameter names another, so no request reads another account's usage; a tenant key, which
    belongs to no account, answers access.not_found. Each meter carries its count, its limit (null where
    none holds), the whole percentage and its state: warning at 80% of the limit, reached at it
    (ADR-0071 d5). Counts only, never content or query text (d7). Reading never blocks: no quota refuses
    this or any other read. Proposed.

    Args:
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        AccountUsage | ErrorEnvelope
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
) -> Response[AccountUsage | ErrorEnvelope]:
    """The calling key's own account's meters against its limits this UTC month.

     Requires: any key of an account, a user root's key (ADR-0071 d1, d6). The account is the key's own
    and no parameter names another, so no request reads another account's usage; a tenant key, which
    belongs to no account, answers access.not_found. Each meter carries its count, its limit (null where
    none holds), the whole percentage and its state: warning at 80% of the limit, reached at it
    (ADR-0071 d5). Counts only, never content or query text (d7). Reading never blocks: no quota refuses
    this or any other read. Proposed.

    Args:
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[AccountUsage | ErrorEnvelope]
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
) -> AccountUsage | ErrorEnvelope | None:
    """The calling key's own account's meters against its limits this UTC month.

     Requires: any key of an account, a user root's key (ADR-0071 d1, d6). The account is the key's own
    and no parameter names another, so no request reads another account's usage; a tenant key, which
    belongs to no account, answers access.not_found. Each meter carries its count, its limit (null where
    none holds), the whole percentage and its state: warning at 80% of the limit, reached at it
    (ADR-0071 d5). Counts only, never content or query text (d7). Reading never blocks: no quota refuses
    this or any other read. Proposed.

    Args:
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        AccountUsage | ErrorEnvelope
    """

    return (
        await asyncio_detailed(
            client=client,
            x_request_id=x_request_id,
            x_correlation_id=x_correlation_id,
            traceparent=traceparent,
        )
    ).parsed
