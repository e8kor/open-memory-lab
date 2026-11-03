from http import HTTPStatus
from typing import Any

import httpx

from ...client import AuthenticatedClient, Client
from ...models.error_envelope import ErrorEnvelope
from ...models.recall_request import RecallRequest
from ...models.recall_response import RecallResponse
from ...types import Response


def _get_kwargs(
    *,
    body: RecallRequest,
) -> dict[str, Any]:
    headers: dict[str, Any] = {}

    _kwargs: dict[str, Any] = {
        "method": "post",
        "url": "/v1/memories/recall",
    }

    _kwargs["json"] = body.to_dict()

    headers["Content-Type"] = "application/json"

    _kwargs["headers"] = headers
    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> ErrorEnvelope | RecallResponse:
    if response.status_code == 200:
        response_200 = RecallResponse.from_dict(response.json())

        return response_200

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
) -> Response[ErrorEnvelope | RecallResponse]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    *,
    client: AuthenticatedClient | Client,
    body: RecallRequest,
) -> Response[ErrorEnvelope | RecallResponse]:
    """Ranked retrieval (FTS + vector) over promoted rows; steering rows precede hits, unranked.

     Requires: reader effective access on the subtree. The scope is the subtree of the body's workarea,
    by id or name, else of the key's default_workarea (ADR-0039 d4); neither answers
    request.workarea_unresolved, with no tenant-wide fallback, and an X-OML-Workarea header is
    request.validation naming /workarea in details.renamed_to (ADR-0039 d7). Read-shaped POST: no
    Idempotency-Key. Applicable steering, the total hit order, own held rows and the max_tokens cut are
    declared in docs/architecture/retrieval.md (ADR-0025 d4, d7, d8). Logged with the ids served after
    truncation and the canonical request digest (D12, ADR-0025 d9). Recall quality unmeasured — FR-4.

    Args:
        body (RecallRequest): The scope is workarea's subtree, else the key's default_workarea's
            (ADR-0039 d4). Unknown fields are refused (request.validation), and so are the designed
            vocabulary filter's terms and expand_terms, declared in openapi-designed.yaml (ADR-0039
            d9); v0.3.0's include_bodies and include_own_held are request.validation naming
            include_text and include_unreviewed in details.renamed_to (ADR-0039 d7).

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | RecallResponse]
    """

    kwargs = _get_kwargs(
        body=body,
    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    *,
    client: AuthenticatedClient | Client,
    body: RecallRequest,
) -> ErrorEnvelope | RecallResponse | None:
    """Ranked retrieval (FTS + vector) over promoted rows; steering rows precede hits, unranked.

     Requires: reader effective access on the subtree. The scope is the subtree of the body's workarea,
    by id or name, else of the key's default_workarea (ADR-0039 d4); neither answers
    request.workarea_unresolved, with no tenant-wide fallback, and an X-OML-Workarea header is
    request.validation naming /workarea in details.renamed_to (ADR-0039 d7). Read-shaped POST: no
    Idempotency-Key. Applicable steering, the total hit order, own held rows and the max_tokens cut are
    declared in docs/architecture/retrieval.md (ADR-0025 d4, d7, d8). Logged with the ids served after
    truncation and the canonical request digest (D12, ADR-0025 d9). Recall quality unmeasured — FR-4.

    Args:
        body (RecallRequest): The scope is workarea's subtree, else the key's default_workarea's
            (ADR-0039 d4). Unknown fields are refused (request.validation), and so are the designed
            vocabulary filter's terms and expand_terms, declared in openapi-designed.yaml (ADR-0039
            d9); v0.3.0's include_bodies and include_own_held are request.validation naming
            include_text and include_unreviewed in details.renamed_to (ADR-0039 d7).

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | RecallResponse
    """

    return sync_detailed(
        client=client,
        body=body,
    ).parsed


async def asyncio_detailed(
    *,
    client: AuthenticatedClient | Client,
    body: RecallRequest,
) -> Response[ErrorEnvelope | RecallResponse]:
    """Ranked retrieval (FTS + vector) over promoted rows; steering rows precede hits, unranked.

     Requires: reader effective access on the subtree. The scope is the subtree of the body's workarea,
    by id or name, else of the key's default_workarea (ADR-0039 d4); neither answers
    request.workarea_unresolved, with no tenant-wide fallback, and an X-OML-Workarea header is
    request.validation naming /workarea in details.renamed_to (ADR-0039 d7). Read-shaped POST: no
    Idempotency-Key. Applicable steering, the total hit order, own held rows and the max_tokens cut are
    declared in docs/architecture/retrieval.md (ADR-0025 d4, d7, d8). Logged with the ids served after
    truncation and the canonical request digest (D12, ADR-0025 d9). Recall quality unmeasured — FR-4.

    Args:
        body (RecallRequest): The scope is workarea's subtree, else the key's default_workarea's
            (ADR-0039 d4). Unknown fields are refused (request.validation), and so are the designed
            vocabulary filter's terms and expand_terms, declared in openapi-designed.yaml (ADR-0039
            d9); v0.3.0's include_bodies and include_own_held are request.validation naming
            include_text and include_unreviewed in details.renamed_to (ADR-0039 d7).

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | RecallResponse]
    """

    kwargs = _get_kwargs(
        body=body,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    *,
    client: AuthenticatedClient | Client,
    body: RecallRequest,
) -> ErrorEnvelope | RecallResponse | None:
    """Ranked retrieval (FTS + vector) over promoted rows; steering rows precede hits, unranked.

     Requires: reader effective access on the subtree. The scope is the subtree of the body's workarea,
    by id or name, else of the key's default_workarea (ADR-0039 d4); neither answers
    request.workarea_unresolved, with no tenant-wide fallback, and an X-OML-Workarea header is
    request.validation naming /workarea in details.renamed_to (ADR-0039 d7). Read-shaped POST: no
    Idempotency-Key. Applicable steering, the total hit order, own held rows and the max_tokens cut are
    declared in docs/architecture/retrieval.md (ADR-0025 d4, d7, d8). Logged with the ids served after
    truncation and the canonical request digest (D12, ADR-0025 d9). Recall quality unmeasured — FR-4.

    Args:
        body (RecallRequest): The scope is workarea's subtree, else the key's default_workarea's
            (ADR-0039 d4). Unknown fields are refused (request.validation), and so are the designed
            vocabulary filter's terms and expand_terms, declared in openapi-designed.yaml (ADR-0039
            d9); v0.3.0's include_bodies and include_own_held are request.validation naming
            include_text and include_unreviewed in details.renamed_to (ADR-0039 d7).

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | RecallResponse
    """

    return (
        await asyncio_detailed(
            client=client,
            body=body,
        )
    ).parsed
