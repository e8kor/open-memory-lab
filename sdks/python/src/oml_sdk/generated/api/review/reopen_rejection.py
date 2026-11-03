from http import HTTPStatus
from typing import Any

import httpx

from ...client import AuthenticatedClient, Client
from ...models.act_result import ActResult
from ...models.error_envelope import ErrorEnvelope
from ...models.reopen_request import ReopenRequest
from ...types import Response


def _get_kwargs(
    *,
    body: ReopenRequest,
    idempotency_key: str,
) -> dict[str, Any]:
    headers: dict[str, Any] = {}
    headers["Idempotency-Key"] = idempotency_key

    _kwargs: dict[str, Any] = {
        "method": "post",
        "url": "/v2/review/reopen",
    }

    _kwargs["json"] = body.to_dict()

    headers["Content-Type"] = "application/json"

    _kwargs["headers"] = headers
    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> ActResult | ErrorEnvelope:
    if response.status_code == 200:
        response_200 = ActResult.from_dict(response.json())

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

    response_default = ErrorEnvelope.from_dict(response.json())

    return response_default


def _build_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Response[ActResult | ErrorEnvelope]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    *,
    client: AuthenticatedClient | Client,
    body: ReopenRequest,
    idempotency_key: str,
) -> Response[ActResult | ErrorEnvelope]:
    """Non-author reopen of a rejection — a journalled verb, not a resubmission.

     Requires: dreamer(review); the original author is refused (review.author_in_lineage). A held_by:
    service key is refused with access.verb_ceiling before any lookup (ADR-0025 d1). A reopen that would
    put a second live version under one claim_key and ordinal is refused with 409
    submission.claim_key_conflict (docs/architecture/audit-and-lineage.md, Supersession chains,
    claim_key and the tip; ADR-0025 d18).

    Args:
        idempotency_key (str):
        body (ReopenRequest):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ActResult | ErrorEnvelope]
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
    body: ReopenRequest,
    idempotency_key: str,
) -> ActResult | ErrorEnvelope | None:
    """Non-author reopen of a rejection — a journalled verb, not a resubmission.

     Requires: dreamer(review); the original author is refused (review.author_in_lineage). A held_by:
    service key is refused with access.verb_ceiling before any lookup (ADR-0025 d1). A reopen that would
    put a second live version under one claim_key and ordinal is refused with 409
    submission.claim_key_conflict (docs/architecture/audit-and-lineage.md, Supersession chains,
    claim_key and the tip; ADR-0025 d18).

    Args:
        idempotency_key (str):
        body (ReopenRequest):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ActResult | ErrorEnvelope
    """

    return sync_detailed(
        client=client,
        body=body,
        idempotency_key=idempotency_key,
    ).parsed


async def asyncio_detailed(
    *,
    client: AuthenticatedClient | Client,
    body: ReopenRequest,
    idempotency_key: str,
) -> Response[ActResult | ErrorEnvelope]:
    """Non-author reopen of a rejection — a journalled verb, not a resubmission.

     Requires: dreamer(review); the original author is refused (review.author_in_lineage). A held_by:
    service key is refused with access.verb_ceiling before any lookup (ADR-0025 d1). A reopen that would
    put a second live version under one claim_key and ordinal is refused with 409
    submission.claim_key_conflict (docs/architecture/audit-and-lineage.md, Supersession chains,
    claim_key and the tip; ADR-0025 d18).

    Args:
        idempotency_key (str):
        body (ReopenRequest):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ActResult | ErrorEnvelope]
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
    body: ReopenRequest,
    idempotency_key: str,
) -> ActResult | ErrorEnvelope | None:
    """Non-author reopen of a rejection — a journalled verb, not a resubmission.

     Requires: dreamer(review); the original author is refused (review.author_in_lineage). A held_by:
    service key is refused with access.verb_ceiling before any lookup (ADR-0025 d1). A reopen that would
    put a second live version under one claim_key and ordinal is refused with 409
    submission.claim_key_conflict (docs/architecture/audit-and-lineage.md, Supersession chains,
    claim_key and the tip; ADR-0025 d18).

    Args:
        idempotency_key (str):
        body (ReopenRequest):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ActResult | ErrorEnvelope
    """

    return (
        await asyncio_detailed(
            client=client,
            body=body,
            idempotency_key=idempotency_key,
        )
    ).parsed
