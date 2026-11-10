from http import HTTPStatus
from typing import Any

import httpx

from ...client import AuthenticatedClient, Client
from ...models.error_envelope import ErrorEnvelope
from ...models.rescan_report import RescanReport
from ...types import UNSET, Response, Unset


def _get_kwargs(
    *,
    after_seq: int | Unset = 0,
    limit: int | Unset = 1000,
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

    params["after_seq"] = after_seq

    params["limit"] = limit

    params = {k: v for k, v in params.items() if v is not UNSET and v is not None}

    _kwargs: dict[str, Any] = {
        "method": "post",
        "url": "/v2/admin/rescan",
        "params": params,
    }

    _kwargs["headers"] = headers
    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> ErrorEnvelope | RescanReport:
    if response.status_code == 200:
        response_200 = RescanReport.from_dict(response.json())

        return response_200

    if response.status_code == 403:
        response_403 = ErrorEnvelope.from_dict(response.json())

        return response_403

    response_default = ErrorEnvelope.from_dict(response.json())

    return response_default


def _build_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Response[ErrorEnvelope | RescanReport]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    *,
    client: AuthenticatedClient | Client,
    after_seq: int | Unset = 0,
    limit: int | Unset = 1000,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> Response[ErrorEnvelope | RescanReport]:
    """Rescan one window as the read does, and record each held version's findings under the server's rule
    set.

     Requires: admin key role (ADR-0065 d13, d15; ADR-0074 d28); every other role answers
    access.verb_ceiling. Answers what the read answers. Each held version of the window takes the
    server's rule set as its scanned_under, and a finding the rescan made becomes its scan finding
    unless it has one; journalled once per window as scan.findings_recorded {scanned_under, versions:
    [{artifact_id, findings: [{class, rule, action, at}]}]}, an empty findings list when there are none,
    when the window holds any held version, else nothing. Inside an in-progress workarea another
    author's held version is served only when scanned under the server's rule set and without a finding.
    Promotes, rejects and deletes nothing. Takes no Idempotency-Key: a repeat reads the window again and
    journals what it read again, and the stored state it leaves is the same. Since v0.6.3.

    Args:
        after_seq (int | Unset):  Default: 0.
        limit (int | Unset):  Default: 1000.
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | RescanReport]
    """

    kwargs = _get_kwargs(
        after_seq=after_seq,
        limit=limit,
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
    after_seq: int | Unset = 0,
    limit: int | Unset = 1000,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> ErrorEnvelope | RescanReport | None:
    """Rescan one window as the read does, and record each held version's findings under the server's rule
    set.

     Requires: admin key role (ADR-0065 d13, d15; ADR-0074 d28); every other role answers
    access.verb_ceiling. Answers what the read answers. Each held version of the window takes the
    server's rule set as its scanned_under, and a finding the rescan made becomes its scan finding
    unless it has one; journalled once per window as scan.findings_recorded {scanned_under, versions:
    [{artifact_id, findings: [{class, rule, action, at}]}]}, an empty findings list when there are none,
    when the window holds any held version, else nothing. Inside an in-progress workarea another
    author's held version is served only when scanned under the server's rule set and without a finding.
    Promotes, rejects and deletes nothing. Takes no Idempotency-Key: a repeat reads the window again and
    journals what it read again, and the stored state it leaves is the same. Since v0.6.3.

    Args:
        after_seq (int | Unset):  Default: 0.
        limit (int | Unset):  Default: 1000.
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | RescanReport
    """

    return sync_detailed(
        client=client,
        after_seq=after_seq,
        limit=limit,
        x_request_id=x_request_id,
        x_correlation_id=x_correlation_id,
        traceparent=traceparent,
    ).parsed


async def asyncio_detailed(
    *,
    client: AuthenticatedClient | Client,
    after_seq: int | Unset = 0,
    limit: int | Unset = 1000,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> Response[ErrorEnvelope | RescanReport]:
    """Rescan one window as the read does, and record each held version's findings under the server's rule
    set.

     Requires: admin key role (ADR-0065 d13, d15; ADR-0074 d28); every other role answers
    access.verb_ceiling. Answers what the read answers. Each held version of the window takes the
    server's rule set as its scanned_under, and a finding the rescan made becomes its scan finding
    unless it has one; journalled once per window as scan.findings_recorded {scanned_under, versions:
    [{artifact_id, findings: [{class, rule, action, at}]}]}, an empty findings list when there are none,
    when the window holds any held version, else nothing. Inside an in-progress workarea another
    author's held version is served only when scanned under the server's rule set and without a finding.
    Promotes, rejects and deletes nothing. Takes no Idempotency-Key: a repeat reads the window again and
    journals what it read again, and the stored state it leaves is the same. Since v0.6.3.

    Args:
        after_seq (int | Unset):  Default: 0.
        limit (int | Unset):  Default: 1000.
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | RescanReport]
    """

    kwargs = _get_kwargs(
        after_seq=after_seq,
        limit=limit,
        x_request_id=x_request_id,
        x_correlation_id=x_correlation_id,
        traceparent=traceparent,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    *,
    client: AuthenticatedClient | Client,
    after_seq: int | Unset = 0,
    limit: int | Unset = 1000,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> ErrorEnvelope | RescanReport | None:
    """Rescan one window as the read does, and record each held version's findings under the server's rule
    set.

     Requires: admin key role (ADR-0065 d13, d15; ADR-0074 d28); every other role answers
    access.verb_ceiling. Answers what the read answers. Each held version of the window takes the
    server's rule set as its scanned_under, and a finding the rescan made becomes its scan finding
    unless it has one; journalled once per window as scan.findings_recorded {scanned_under, versions:
    [{artifact_id, findings: [{class, rule, action, at}]}]}, an empty findings list when there are none,
    when the window holds any held version, else nothing. Inside an in-progress workarea another
    author's held version is served only when scanned under the server's rule set and without a finding.
    Promotes, rejects and deletes nothing. Takes no Idempotency-Key: a repeat reads the window again and
    journals what it read again, and the stored state it leaves is the same. Since v0.6.3.

    Args:
        after_seq (int | Unset):  Default: 0.
        limit (int | Unset):  Default: 1000.
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | RescanReport
    """

    return (
        await asyncio_detailed(
            client=client,
            after_seq=after_seq,
            limit=limit,
            x_request_id=x_request_id,
            x_correlation_id=x_correlation_id,
            traceparent=traceparent,
        )
    ).parsed
