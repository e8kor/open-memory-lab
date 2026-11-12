from http import HTTPStatus
from typing import Any

import httpx

from ...client import AuthenticatedClient, Client
from ...models.admin_usage_ledger_format import AdminUsageLedgerFormat
from ...models.error_envelope import ErrorEnvelope
from ...models.usage_ledger import UsageLedger
from ...types import UNSET, Response, Unset


def _get_kwargs(
    *,
    period: str,
    format_: AdminUsageLedgerFormat | Unset = AdminUsageLedgerFormat.JSON,
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

    params["period"] = period

    json_format_: str | Unset = UNSET
    if not isinstance(format_, Unset):
        json_format_ = format_.value

    params["format"] = json_format_

    params = {k: v for k, v in params.items() if v is not UNSET and v is not None}

    _kwargs: dict[str, Any] = {
        "method": "get",
        "url": "/v2/admin/usage",
        "params": params,
    }

    _kwargs["headers"] = headers
    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> ErrorEnvelope | UsageLedger:
    if response.status_code == 200:
        response_200 = UsageLedger.from_dict(response.json())

        return response_200

    if response.status_code == 400:
        response_400 = ErrorEnvelope.from_dict(response.json())

        return response_400

    if response.status_code == 403:
        response_403 = ErrorEnvelope.from_dict(response.json())

        return response_403

    response_default = ErrorEnvelope.from_dict(response.json())

    return response_default


def _build_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Response[ErrorEnvelope | UsageLedger]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    *,
    client: AuthenticatedClient | Client,
    period: str,
    format_: AdminUsageLedgerFormat | Unset = AdminUsageLedgerFormat.JSON,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> Response[ErrorEnvelope | UsageLedger]:
    """The usage ledger of one UTC month, every account, one row per account per closed day, as JSON or
    CSV.

     Requires: admin key role (ADR-0071 d4, d6). period is the month, YYYY-MM, and required; format is
    json, the default, or csv, whose header is
    day,account,user,profile,memories,text_bytes,writes,recalls,keys,connections. A row holds the stocks
    as the hourly rollup read them after the day closed and that day's flows; profile is the one the
    account was on when the day closed: the one an admin assigned, else the sign-up profile in the sign-
    up tenant, and null for an account under no limits. Rows are kept 25 months. Counts only (d7).
    Journals nothing. Proposed.

    Args:
        period (str):
        format_ (AdminUsageLedgerFormat | Unset):  Default: AdminUsageLedgerFormat.JSON.
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | UsageLedger]
    """

    kwargs = _get_kwargs(
        period=period,
        format_=format_,
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
    period: str,
    format_: AdminUsageLedgerFormat | Unset = AdminUsageLedgerFormat.JSON,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> ErrorEnvelope | UsageLedger | None:
    """The usage ledger of one UTC month, every account, one row per account per closed day, as JSON or
    CSV.

     Requires: admin key role (ADR-0071 d4, d6). period is the month, YYYY-MM, and required; format is
    json, the default, or csv, whose header is
    day,account,user,profile,memories,text_bytes,writes,recalls,keys,connections. A row holds the stocks
    as the hourly rollup read them after the day closed and that day's flows; profile is the one the
    account was on when the day closed: the one an admin assigned, else the sign-up profile in the sign-
    up tenant, and null for an account under no limits. Rows are kept 25 months. Counts only (d7).
    Journals nothing. Proposed.

    Args:
        period (str):
        format_ (AdminUsageLedgerFormat | Unset):  Default: AdminUsageLedgerFormat.JSON.
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | UsageLedger
    """

    return sync_detailed(
        client=client,
        period=period,
        format_=format_,
        x_request_id=x_request_id,
        x_correlation_id=x_correlation_id,
        traceparent=traceparent,
    ).parsed


async def asyncio_detailed(
    *,
    client: AuthenticatedClient | Client,
    period: str,
    format_: AdminUsageLedgerFormat | Unset = AdminUsageLedgerFormat.JSON,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> Response[ErrorEnvelope | UsageLedger]:
    """The usage ledger of one UTC month, every account, one row per account per closed day, as JSON or
    CSV.

     Requires: admin key role (ADR-0071 d4, d6). period is the month, YYYY-MM, and required; format is
    json, the default, or csv, whose header is
    day,account,user,profile,memories,text_bytes,writes,recalls,keys,connections. A row holds the stocks
    as the hourly rollup read them after the day closed and that day's flows; profile is the one the
    account was on when the day closed: the one an admin assigned, else the sign-up profile in the sign-
    up tenant, and null for an account under no limits. Rows are kept 25 months. Counts only (d7).
    Journals nothing. Proposed.

    Args:
        period (str):
        format_ (AdminUsageLedgerFormat | Unset):  Default: AdminUsageLedgerFormat.JSON.
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | UsageLedger]
    """

    kwargs = _get_kwargs(
        period=period,
        format_=format_,
        x_request_id=x_request_id,
        x_correlation_id=x_correlation_id,
        traceparent=traceparent,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    *,
    client: AuthenticatedClient | Client,
    period: str,
    format_: AdminUsageLedgerFormat | Unset = AdminUsageLedgerFormat.JSON,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> ErrorEnvelope | UsageLedger | None:
    """The usage ledger of one UTC month, every account, one row per account per closed day, as JSON or
    CSV.

     Requires: admin key role (ADR-0071 d4, d6). period is the month, YYYY-MM, and required; format is
    json, the default, or csv, whose header is
    day,account,user,profile,memories,text_bytes,writes,recalls,keys,connections. A row holds the stocks
    as the hourly rollup read them after the day closed and that day's flows; profile is the one the
    account was on when the day closed: the one an admin assigned, else the sign-up profile in the sign-
    up tenant, and null for an account under no limits. Rows are kept 25 months. Counts only (d7).
    Journals nothing. Proposed.

    Args:
        period (str):
        format_ (AdminUsageLedgerFormat | Unset):  Default: AdminUsageLedgerFormat.JSON.
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | UsageLedger
    """

    return (
        await asyncio_detailed(
            client=client,
            period=period,
            format_=format_,
            x_request_id=x_request_id,
            x_correlation_id=x_correlation_id,
            traceparent=traceparent,
        )
    ).parsed
