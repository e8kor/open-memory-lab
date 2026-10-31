from http import HTTPStatus
from typing import Any
from urllib.parse import quote

import httpx

from ...client import AuthenticatedClient, Client
from ...models.claim_edge import ClaimEdge
from ...models.error_envelope import ErrorEnvelope
from ...types import Response


def _get_kwargs(
    edge_id: str,
    *,
    idempotency_key: str,
) -> dict[str, Any]:
    headers: dict[str, Any] = {}
    headers["Idempotency-Key"] = idempotency_key

    _kwargs: dict[str, Any] = {
        "method": "post",
        "url": "/v2/claim-edges/{edge_id}/ratify".format(
            edge_id=quote(str(edge_id), safe=""),
        ),
    }

    _kwargs["headers"] = headers
    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> ClaimEdge | ErrorEnvelope:
    if response.status_code == 200:
        response_200 = ClaimEdge.from_dict(response.json())

        return response_200

    if response.status_code == 403:
        response_403 = ErrorEnvelope.from_dict(response.json())

        return response_403

    response_default = ErrorEnvelope.from_dict(response.json())

    return response_default


def _build_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Response[ClaimEdge | ErrorEnvelope]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    edge_id: str,
    *,
    client: AuthenticatedClient | Client,
    idempotency_key: str,
) -> Response[ClaimEdge | ErrorEnvelope]:
    """Ratify a proposed claim edge.

     Requires: dreamer(review); never the proposer (review.ratifier_is_proposer) and refused on lineage
    collision (review.author_in_lineage) — R2. A held_by: service key is refused with
    access.verb_ceiling before any lookup (ADR-0025 d1).

    Args:
        edge_id (str):
        idempotency_key (str):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ClaimEdge | ErrorEnvelope]
    """

    kwargs = _get_kwargs(
        edge_id=edge_id,
        idempotency_key=idempotency_key,
    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    edge_id: str,
    *,
    client: AuthenticatedClient | Client,
    idempotency_key: str,
) -> ClaimEdge | ErrorEnvelope | None:
    """Ratify a proposed claim edge.

     Requires: dreamer(review); never the proposer (review.ratifier_is_proposer) and refused on lineage
    collision (review.author_in_lineage) — R2. A held_by: service key is refused with
    access.verb_ceiling before any lookup (ADR-0025 d1).

    Args:
        edge_id (str):
        idempotency_key (str):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ClaimEdge | ErrorEnvelope
    """

    return sync_detailed(
        edge_id=edge_id,
        client=client,
        idempotency_key=idempotency_key,
    ).parsed


async def asyncio_detailed(
    edge_id: str,
    *,
    client: AuthenticatedClient | Client,
    idempotency_key: str,
) -> Response[ClaimEdge | ErrorEnvelope]:
    """Ratify a proposed claim edge.

     Requires: dreamer(review); never the proposer (review.ratifier_is_proposer) and refused on lineage
    collision (review.author_in_lineage) — R2. A held_by: service key is refused with
    access.verb_ceiling before any lookup (ADR-0025 d1).

    Args:
        edge_id (str):
        idempotency_key (str):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ClaimEdge | ErrorEnvelope]
    """

    kwargs = _get_kwargs(
        edge_id=edge_id,
        idempotency_key=idempotency_key,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    edge_id: str,
    *,
    client: AuthenticatedClient | Client,
    idempotency_key: str,
) -> ClaimEdge | ErrorEnvelope | None:
    """Ratify a proposed claim edge.

     Requires: dreamer(review); never the proposer (review.ratifier_is_proposer) and refused on lineage
    collision (review.author_in_lineage) — R2. A held_by: service key is refused with
    access.verb_ceiling before any lookup (ADR-0025 d1).

    Args:
        edge_id (str):
        idempotency_key (str):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ClaimEdge | ErrorEnvelope
    """

    return (
        await asyncio_detailed(
            edge_id=edge_id,
            client=client,
            idempotency_key=idempotency_key,
        )
    ).parsed
