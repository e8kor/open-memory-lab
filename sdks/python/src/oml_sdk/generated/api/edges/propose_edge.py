from http import HTTPStatus
from typing import Any

import httpx

from ...client import AuthenticatedClient, Client
from ...models.brief_edge import BriefEdge
from ...models.brief_edge_proposal import BriefEdgeProposal
from ...models.claim_edge import ClaimEdge
from ...models.claim_edge_proposal import ClaimEdgeProposal
from ...models.error_envelope import ErrorEnvelope
from ...types import Response


def _get_kwargs(
    *,
    body: BriefEdgeProposal | ClaimEdgeProposal,
    idempotency_key: str,
) -> dict[str, Any]:
    headers: dict[str, Any] = {}
    headers["Idempotency-Key"] = idempotency_key

    _kwargs: dict[str, Any] = {
        "method": "post",
        "url": "/v2/edges",
    }

    if isinstance(body, ClaimEdgeProposal):
        _kwargs["json"] = body.to_dict()
    else:
        _kwargs["json"] = body.to_dict()

    headers["Content-Type"] = "application/json"

    _kwargs["headers"] = headers
    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> BriefEdge | ClaimEdge | ErrorEnvelope:
    if response.status_code == 202:

        def _parse_response_202(data: object) -> BriefEdge | ClaimEdge:
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

        response_202 = _parse_response_202(response.json())

        return response_202

    if response.status_code == 415:
        response_415 = ErrorEnvelope.from_dict(response.json())

        return response_415

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
    *,
    client: AuthenticatedClient | Client,
    body: BriefEdgeProposal | ClaimEdgeProposal,
    idempotency_key: str,
) -> Response[BriefEdge | ClaimEdge | ErrorEnvelope]:
    """Propose a claim-to-claim edge or a claim's place in a workarea's brief; pending until ratified.

     kind claim requires dreamer(propose); its label is open, with supports, contradicts, relates_to and
    continues recommended and oml: reserved (ADR-0046 d6). kind brief requires dreamer effective access
    on the workarea and a promoted served tip the key reads; its label is brief and is not sent
    (ADR-0046 d7, d8). kind workarea, designed and not served, requires dreamer(propose) effective
    access on at least one endpoint and reader on both (edge-authority rule, D7); an admin who must act
    here holds a separate dreamer key, since a key never combines admin with dreamer grants
    (access.role_exclusive). Each kind journals its own proposal event (ADR-0039 d9).

    Args:
        idempotency_key (str):
        body (BriefEdgeProposal | ClaimEdgeProposal): One proposal body for the served edge kinds,
            chosen by kind (ADR-0039 d9; ADR-0046 d7).

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[BriefEdge | ClaimEdge | ErrorEnvelope]
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
    body: BriefEdgeProposal | ClaimEdgeProposal,
    idempotency_key: str,
) -> BriefEdge | ClaimEdge | ErrorEnvelope | None:
    """Propose a claim-to-claim edge or a claim's place in a workarea's brief; pending until ratified.

     kind claim requires dreamer(propose); its label is open, with supports, contradicts, relates_to and
    continues recommended and oml: reserved (ADR-0046 d6). kind brief requires dreamer effective access
    on the workarea and a promoted served tip the key reads; its label is brief and is not sent
    (ADR-0046 d7, d8). kind workarea, designed and not served, requires dreamer(propose) effective
    access on at least one endpoint and reader on both (edge-authority rule, D7); an admin who must act
    here holds a separate dreamer key, since a key never combines admin with dreamer grants
    (access.role_exclusive). Each kind journals its own proposal event (ADR-0039 d9).

    Args:
        idempotency_key (str):
        body (BriefEdgeProposal | ClaimEdgeProposal): One proposal body for the served edge kinds,
            chosen by kind (ADR-0039 d9; ADR-0046 d7).

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        BriefEdge | ClaimEdge | ErrorEnvelope
    """

    return sync_detailed(
        client=client,
        body=body,
        idempotency_key=idempotency_key,
    ).parsed


async def asyncio_detailed(
    *,
    client: AuthenticatedClient | Client,
    body: BriefEdgeProposal | ClaimEdgeProposal,
    idempotency_key: str,
) -> Response[BriefEdge | ClaimEdge | ErrorEnvelope]:
    """Propose a claim-to-claim edge or a claim's place in a workarea's brief; pending until ratified.

     kind claim requires dreamer(propose); its label is open, with supports, contradicts, relates_to and
    continues recommended and oml: reserved (ADR-0046 d6). kind brief requires dreamer effective access
    on the workarea and a promoted served tip the key reads; its label is brief and is not sent
    (ADR-0046 d7, d8). kind workarea, designed and not served, requires dreamer(propose) effective
    access on at least one endpoint and reader on both (edge-authority rule, D7); an admin who must act
    here holds a separate dreamer key, since a key never combines admin with dreamer grants
    (access.role_exclusive). Each kind journals its own proposal event (ADR-0039 d9).

    Args:
        idempotency_key (str):
        body (BriefEdgeProposal | ClaimEdgeProposal): One proposal body for the served edge kinds,
            chosen by kind (ADR-0039 d9; ADR-0046 d7).

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[BriefEdge | ClaimEdge | ErrorEnvelope]
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
    body: BriefEdgeProposal | ClaimEdgeProposal,
    idempotency_key: str,
) -> BriefEdge | ClaimEdge | ErrorEnvelope | None:
    """Propose a claim-to-claim edge or a claim's place in a workarea's brief; pending until ratified.

     kind claim requires dreamer(propose); its label is open, with supports, contradicts, relates_to and
    continues recommended and oml: reserved (ADR-0046 d6). kind brief requires dreamer effective access
    on the workarea and a promoted served tip the key reads; its label is brief and is not sent
    (ADR-0046 d7, d8). kind workarea, designed and not served, requires dreamer(propose) effective
    access on at least one endpoint and reader on both (edge-authority rule, D7); an admin who must act
    here holds a separate dreamer key, since a key never combines admin with dreamer grants
    (access.role_exclusive). Each kind journals its own proposal event (ADR-0039 d9).

    Args:
        idempotency_key (str):
        body (BriefEdgeProposal | ClaimEdgeProposal): One proposal body for the served edge kinds,
            chosen by kind (ADR-0039 d9; ADR-0046 d7).

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        BriefEdge | ClaimEdge | ErrorEnvelope
    """

    return (
        await asyncio_detailed(
            client=client,
            body=body,
            idempotency_key=idempotency_key,
        )
    ).parsed
