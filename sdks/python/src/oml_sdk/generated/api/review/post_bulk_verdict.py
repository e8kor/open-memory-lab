from http import HTTPStatus
from typing import Any

import httpx

from ...client import AuthenticatedClient, Client
from ...models.bulk_verdict_request import BulkVerdictRequest
from ...models.bulk_verdict_result import BulkVerdictResult
from ...models.error_envelope import ErrorEnvelope
from ...types import UNSET, Response, Unset


def _get_kwargs(
    *,
    body: BulkVerdictRequest,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
    idempotency_key: str,
) -> dict[str, Any]:
    headers: dict[str, Any] = {}
    if not isinstance(x_request_id, Unset):
        headers["X-Request-Id"] = x_request_id

    if not isinstance(x_correlation_id, Unset):
        headers["X-Correlation-Id"] = x_correlation_id

    if not isinstance(traceparent, Unset):
        headers["traceparent"] = traceparent

    headers["Idempotency-Key"] = idempotency_key

    _kwargs: dict[str, Any] = {
        "method": "post",
        "url": "/v2/review/verdicts/bulk",
    }

    _kwargs["json"] = body.to_dict()

    headers["Content-Type"] = "application/json"

    _kwargs["headers"] = headers
    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> BulkVerdictResult | ErrorEnvelope:
    if response.status_code == 200:
        response_200 = BulkVerdictResult.from_dict(response.json())

        return response_200

    if response.status_code == 403:
        response_403 = ErrorEnvelope.from_dict(response.json())

        return response_403

    if response.status_code == 415:
        response_415 = ErrorEnvelope.from_dict(response.json())

        return response_415

    if response.status_code == 429:
        response_429 = ErrorEnvelope.from_dict(response.json())

        return response_429

    response_default = ErrorEnvelope.from_dict(response.json())

    return response_default


def _build_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Response[BulkVerdictResult | ErrorEnvelope]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    *,
    client: AuthenticatedClient | Client,
    body: BulkVerdictRequest,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
    idempotency_key: str,
) -> Response[BulkVerdictResult | ErrorEnvelope]:
    """One verdict per named held item, each judged, journalled and signed as a single verdict; outcomes
    per item.

     Requires: a dreamer key held_by human (access.verb_ceiling, details.reason
    bulk_review_is_human_only, before any lookup; ADR-0055 d1). artifact_ids names 1 to 200 distinct ids
    the reviewer was listed; a filter is never evaluated (d3). Each id runs the verdict of POST
    /v2/review/verdicts in the order named, in its own transaction, with its own review.approve or
    review.reject and, for an approval, its own lifecycle.promote or lifecycle.supersede carrying
    same_user (d4, d7). An item the single verdict would refuse is skipped with that refusal's code:
    review.author_in_lineage (R2), review.serve_invalid (no longer held), submission.tip_conflict,
    access.not_found, access.grant_refused, workarea.frozen; nothing already committed is undone (d5,
    d6). Each item records under the Idempotency-Key and its own id: a retry replays decided items with
    replayed true and judges the rest; another verdict or reason for a decided item is
    idempotency.digest_mismatch on that item (d12). reason is required to reject and journalled on each
    item (d8).

    Args:
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        idempotency_key (str):
        body (BulkVerdictRequest):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[BulkVerdictResult | ErrorEnvelope]
    """

    kwargs = _get_kwargs(
        body=body,
        x_request_id=x_request_id,
        x_correlation_id=x_correlation_id,
        traceparent=traceparent,
        idempotency_key=idempotency_key,
    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    *,
    client: AuthenticatedClient | Client,
    body: BulkVerdictRequest,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
    idempotency_key: str,
) -> BulkVerdictResult | ErrorEnvelope | None:
    """One verdict per named held item, each judged, journalled and signed as a single verdict; outcomes
    per item.

     Requires: a dreamer key held_by human (access.verb_ceiling, details.reason
    bulk_review_is_human_only, before any lookup; ADR-0055 d1). artifact_ids names 1 to 200 distinct ids
    the reviewer was listed; a filter is never evaluated (d3). Each id runs the verdict of POST
    /v2/review/verdicts in the order named, in its own transaction, with its own review.approve or
    review.reject and, for an approval, its own lifecycle.promote or lifecycle.supersede carrying
    same_user (d4, d7). An item the single verdict would refuse is skipped with that refusal's code:
    review.author_in_lineage (R2), review.serve_invalid (no longer held), submission.tip_conflict,
    access.not_found, access.grant_refused, workarea.frozen; nothing already committed is undone (d5,
    d6). Each item records under the Idempotency-Key and its own id: a retry replays decided items with
    replayed true and judges the rest; another verdict or reason for a decided item is
    idempotency.digest_mismatch on that item (d12). reason is required to reject and journalled on each
    item (d8).

    Args:
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        idempotency_key (str):
        body (BulkVerdictRequest):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        BulkVerdictResult | ErrorEnvelope
    """

    return sync_detailed(
        client=client,
        body=body,
        x_request_id=x_request_id,
        x_correlation_id=x_correlation_id,
        traceparent=traceparent,
        idempotency_key=idempotency_key,
    ).parsed


async def asyncio_detailed(
    *,
    client: AuthenticatedClient | Client,
    body: BulkVerdictRequest,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
    idempotency_key: str,
) -> Response[BulkVerdictResult | ErrorEnvelope]:
    """One verdict per named held item, each judged, journalled and signed as a single verdict; outcomes
    per item.

     Requires: a dreamer key held_by human (access.verb_ceiling, details.reason
    bulk_review_is_human_only, before any lookup; ADR-0055 d1). artifact_ids names 1 to 200 distinct ids
    the reviewer was listed; a filter is never evaluated (d3). Each id runs the verdict of POST
    /v2/review/verdicts in the order named, in its own transaction, with its own review.approve or
    review.reject and, for an approval, its own lifecycle.promote or lifecycle.supersede carrying
    same_user (d4, d7). An item the single verdict would refuse is skipped with that refusal's code:
    review.author_in_lineage (R2), review.serve_invalid (no longer held), submission.tip_conflict,
    access.not_found, access.grant_refused, workarea.frozen; nothing already committed is undone (d5,
    d6). Each item records under the Idempotency-Key and its own id: a retry replays decided items with
    replayed true and judges the rest; another verdict or reason for a decided item is
    idempotency.digest_mismatch on that item (d12). reason is required to reject and journalled on each
    item (d8).

    Args:
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        idempotency_key (str):
        body (BulkVerdictRequest):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[BulkVerdictResult | ErrorEnvelope]
    """

    kwargs = _get_kwargs(
        body=body,
        x_request_id=x_request_id,
        x_correlation_id=x_correlation_id,
        traceparent=traceparent,
        idempotency_key=idempotency_key,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    *,
    client: AuthenticatedClient | Client,
    body: BulkVerdictRequest,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
    idempotency_key: str,
) -> BulkVerdictResult | ErrorEnvelope | None:
    """One verdict per named held item, each judged, journalled and signed as a single verdict; outcomes
    per item.

     Requires: a dreamer key held_by human (access.verb_ceiling, details.reason
    bulk_review_is_human_only, before any lookup; ADR-0055 d1). artifact_ids names 1 to 200 distinct ids
    the reviewer was listed; a filter is never evaluated (d3). Each id runs the verdict of POST
    /v2/review/verdicts in the order named, in its own transaction, with its own review.approve or
    review.reject and, for an approval, its own lifecycle.promote or lifecycle.supersede carrying
    same_user (d4, d7). An item the single verdict would refuse is skipped with that refusal's code:
    review.author_in_lineage (R2), review.serve_invalid (no longer held), submission.tip_conflict,
    access.not_found, access.grant_refused, workarea.frozen; nothing already committed is undone (d5,
    d6). Each item records under the Idempotency-Key and its own id: a retry replays decided items with
    replayed true and judges the rest; another verdict or reason for a decided item is
    idempotency.digest_mismatch on that item (d12). reason is required to reject and journalled on each
    item (d8).

    Args:
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        idempotency_key (str):
        body (BulkVerdictRequest):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        BulkVerdictResult | ErrorEnvelope
    """

    return (
        await asyncio_detailed(
            client=client,
            body=body,
            x_request_id=x_request_id,
            x_correlation_id=x_correlation_id,
            traceparent=traceparent,
            idempotency_key=idempotency_key,
        )
    ).parsed
