from http import HTTPStatus
from typing import Any
from urllib.parse import quote

import httpx

from ...client import AuthenticatedClient, Client
from ...models.brief_edge import BriefEdge
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
        "url": "/v2/edges/{edge_id}/ratify".format(
            edge_id=quote(str(edge_id), safe=""),
        ),
    }

    _kwargs["headers"] = headers
    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> BriefEdge | ClaimEdge | ErrorEnvelope:
    if response.status_code == 200:

        def _parse_response_200(data: object) -> BriefEdge | ClaimEdge:
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                componentsschemas_edge_type_0 = ClaimEdge.from_dict(data)

                return componentsschemas_edge_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            if not isinstance(data, dict):
                raise TypeError()
            componentsschemas_edge_type_1 = BriefEdge.from_dict(data)

            return componentsschemas_edge_type_1

        response_200 = _parse_response_200(response.json())

        return response_200

    if response.status_code == 403:
        response_403 = ErrorEnvelope.from_dict(response.json())

        return response_403

    response_default = ErrorEnvelope.from_dict(response.json())

    return response_default


def _build_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Response[BriefEdge | ClaimEdge | ErrorEnvelope]:
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
) -> Response[BriefEdge | ClaimEdge | ErrorEnvelope]:
    """Ratify a proposed edge of either kind.

     The edge's kind follows from edge_id. kind brief requires dreamer effective access on the workarea
    and refuses the proposer (review.ratifier_is_proposer); a decided edge is review.serve_invalid
    consumed (ADR-0046 d8). kind claim requires dreamer(review), and refuses the proposer
    (review.ratifier_is_proposer) and a lineage collision (review.author_in_lineage) — R2. kind workarea
    requires dreamer(review) effective access on BOTH endpoints and refuses the proposer
    (review.ratifier_is_proposer — R2 extends to edges). A held_by: service key is refused with
    access.verb_ceiling before any lookup (ADR-0025 d1). Each kind journals its own ratification event
    (ADR-0039 d9).

    Args:
        edge_id (str):
        idempotency_key (str):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[BriefEdge | ClaimEdge | ErrorEnvelope]
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
) -> BriefEdge | ClaimEdge | ErrorEnvelope | None:
    """Ratify a proposed edge of either kind.

     The edge's kind follows from edge_id. kind brief requires dreamer effective access on the workarea
    and refuses the proposer (review.ratifier_is_proposer); a decided edge is review.serve_invalid
    consumed (ADR-0046 d8). kind claim requires dreamer(review), and refuses the proposer
    (review.ratifier_is_proposer) and a lineage collision (review.author_in_lineage) — R2. kind workarea
    requires dreamer(review) effective access on BOTH endpoints and refuses the proposer
    (review.ratifier_is_proposer — R2 extends to edges). A held_by: service key is refused with
    access.verb_ceiling before any lookup (ADR-0025 d1). Each kind journals its own ratification event
    (ADR-0039 d9).

    Args:
        edge_id (str):
        idempotency_key (str):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        BriefEdge | ClaimEdge | ErrorEnvelope
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
) -> Response[BriefEdge | ClaimEdge | ErrorEnvelope]:
    """Ratify a proposed edge of either kind.

     The edge's kind follows from edge_id. kind brief requires dreamer effective access on the workarea
    and refuses the proposer (review.ratifier_is_proposer); a decided edge is review.serve_invalid
    consumed (ADR-0046 d8). kind claim requires dreamer(review), and refuses the proposer
    (review.ratifier_is_proposer) and a lineage collision (review.author_in_lineage) — R2. kind workarea
    requires dreamer(review) effective access on BOTH endpoints and refuses the proposer
    (review.ratifier_is_proposer — R2 extends to edges). A held_by: service key is refused with
    access.verb_ceiling before any lookup (ADR-0025 d1). Each kind journals its own ratification event
    (ADR-0039 d9).

    Args:
        edge_id (str):
        idempotency_key (str):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[BriefEdge | ClaimEdge | ErrorEnvelope]
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
) -> BriefEdge | ClaimEdge | ErrorEnvelope | None:
    """Ratify a proposed edge of either kind.

     The edge's kind follows from edge_id. kind brief requires dreamer effective access on the workarea
    and refuses the proposer (review.ratifier_is_proposer); a decided edge is review.serve_invalid
    consumed (ADR-0046 d8). kind claim requires dreamer(review), and refuses the proposer
    (review.ratifier_is_proposer) and a lineage collision (review.author_in_lineage) — R2. kind workarea
    requires dreamer(review) effective access on BOTH endpoints and refuses the proposer
    (review.ratifier_is_proposer — R2 extends to edges). A held_by: service key is refused with
    access.verb_ceiling before any lookup (ADR-0025 d1). Each kind journals its own ratification event
    (ADR-0039 d9).

    Args:
        edge_id (str):
        idempotency_key (str):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        BriefEdge | ClaimEdge | ErrorEnvelope
    """

    return (
        await asyncio_detailed(
            edge_id=edge_id,
            client=client,
            idempotency_key=idempotency_key,
        )
    ).parsed
