from http import HTTPStatus
from typing import Any

import httpx

from ...client import AuthenticatedClient, Client
from ...models.error_envelope import ErrorEnvelope
from ...models.verdict_request import VerdictRequest
from ...models.verdict_result import VerdictResult
from ...types import Response


def _get_kwargs(
    *,
    body: VerdictRequest,
    idempotency_key: str,
) -> dict[str, Any]:
    headers: dict[str, Any] = {}
    headers["Idempotency-Key"] = idempotency_key

    _kwargs: dict[str, Any] = {
        "method": "post",
        "url": "/v2/review/verdicts",
    }

    _kwargs["json"] = body.to_dict()

    headers["Content-Type"] = "application/json"

    _kwargs["headers"] = headers
    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> ErrorEnvelope | VerdictResult:
    if response.status_code == 200:
        response_200 = VerdictResult.from_dict(response.json())

        return response_200

    if response.status_code == 403:
        response_403 = ErrorEnvelope.from_dict(response.json())

        return response_403

    if response.status_code == 409:
        response_409 = ErrorEnvelope.from_dict(response.json())

        return response_409

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
) -> Response[ErrorEnvelope | VerdictResult]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    *,
    client: AuthenticatedClient | Client,
    body: VerdictRequest,
    idempotency_key: str,
) -> Response[ErrorEnvelope | VerdictResult]:
    """Verdict against a valid serve; the journalled verdict records the served digests.

     Requires: dreamer(review), valid unexpired unconsumed serve bound to this key (review.serve_invalid
    names the failure), and dreamer effective access on the row's workarea, resolved by the refusal
    order of docs/architecture/roles-and-keys.md: access.grant_refused with needed_role dreamer where
    reader is effective there, access.not_found otherwise (ADR-0025 d1); a held_by: service key is
    refused with access.verb_ceiling before any lookup. An approve that would ratify a supersession
    whose target is no longer the tip of the successor's own workarea and claim_key is refused with 409
    submission.tip_conflict (details.current_tip when readable); nothing is journalled and the serve_id
    is not consumed (ADR-0025 d10). Resulting state: promoted, held (more verdicts needed) or rejected.

    Args:
        idempotency_key (str):
        body (VerdictRequest):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | VerdictResult]
    """

    kwargs = _get_kwargs(
        body=body,
        idempotency_key=idempotency_key,
    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    *,
    client: AuthenticatedClient | Client,
    body: VerdictRequest,
    idempotency_key: str,
) -> ErrorEnvelope | VerdictResult | None:
    """Verdict against a valid serve; the journalled verdict records the served digests.

     Requires: dreamer(review), valid unexpired unconsumed serve bound to this key (review.serve_invalid
    names the failure), and dreamer effective access on the row's workarea, resolved by the refusal
    order of docs/architecture/roles-and-keys.md: access.grant_refused with needed_role dreamer where
    reader is effective there, access.not_found otherwise (ADR-0025 d1); a held_by: service key is
    refused with access.verb_ceiling before any lookup. An approve that would ratify a supersession
    whose target is no longer the tip of the successor's own workarea and claim_key is refused with 409
    submission.tip_conflict (details.current_tip when readable); nothing is journalled and the serve_id
    is not consumed (ADR-0025 d10). Resulting state: promoted, held (more verdicts needed) or rejected.

    Args:
        idempotency_key (str):
        body (VerdictRequest):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | VerdictResult
    """

    return sync_detailed(
        client=client,
        body=body,
        idempotency_key=idempotency_key,
    ).parsed


async def asyncio_detailed(
    *,
    client: AuthenticatedClient | Client,
    body: VerdictRequest,
    idempotency_key: str,
) -> Response[ErrorEnvelope | VerdictResult]:
    """Verdict against a valid serve; the journalled verdict records the served digests.

     Requires: dreamer(review), valid unexpired unconsumed serve bound to this key (review.serve_invalid
    names the failure), and dreamer effective access on the row's workarea, resolved by the refusal
    order of docs/architecture/roles-and-keys.md: access.grant_refused with needed_role dreamer where
    reader is effective there, access.not_found otherwise (ADR-0025 d1); a held_by: service key is
    refused with access.verb_ceiling before any lookup. An approve that would ratify a supersession
    whose target is no longer the tip of the successor's own workarea and claim_key is refused with 409
    submission.tip_conflict (details.current_tip when readable); nothing is journalled and the serve_id
    is not consumed (ADR-0025 d10). Resulting state: promoted, held (more verdicts needed) or rejected.

    Args:
        idempotency_key (str):
        body (VerdictRequest):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | VerdictResult]
    """

    kwargs = _get_kwargs(
        body=body,
        idempotency_key=idempotency_key,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    *,
    client: AuthenticatedClient | Client,
    body: VerdictRequest,
    idempotency_key: str,
) -> ErrorEnvelope | VerdictResult | None:
    """Verdict against a valid serve; the journalled verdict records the served digests.

     Requires: dreamer(review), valid unexpired unconsumed serve bound to this key (review.serve_invalid
    names the failure), and dreamer effective access on the row's workarea, resolved by the refusal
    order of docs/architecture/roles-and-keys.md: access.grant_refused with needed_role dreamer where
    reader is effective there, access.not_found otherwise (ADR-0025 d1); a held_by: service key is
    refused with access.verb_ceiling before any lookup. An approve that would ratify a supersession
    whose target is no longer the tip of the successor's own workarea and claim_key is refused with 409
    submission.tip_conflict (details.current_tip when readable); nothing is journalled and the serve_id
    is not consumed (ADR-0025 d10). Resulting state: promoted, held (more verdicts needed) or rejected.

    Args:
        idempotency_key (str):
        body (VerdictRequest):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | VerdictResult
    """

    return (
        await asyncio_detailed(
            client=client,
            body=body,
            idempotency_key=idempotency_key,
        )
    ).parsed
