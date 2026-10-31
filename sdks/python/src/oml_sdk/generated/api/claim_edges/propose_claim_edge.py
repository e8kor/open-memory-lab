from http import HTTPStatus
from typing import Any

import httpx

from ...client import AuthenticatedClient, Client
from ...models.claim_edge import ClaimEdge
from ...models.claim_edge_proposal import ClaimEdgeProposal
from ...models.error_envelope import ErrorEnvelope
from ...types import Response


def _get_kwargs(
    *,
    body: ClaimEdgeProposal,
    idempotency_key: str,
) -> dict[str, Any]:
    headers: dict[str, Any] = {}
    headers["Idempotency-Key"] = idempotency_key

    _kwargs: dict[str, Any] = {
        "method": "post",
        "url": "/v2/claim-edges",
    }

    _kwargs["json"] = body.to_dict()

    headers["Content-Type"] = "application/json"

    _kwargs["headers"] = headers
    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> ClaimEdge | ErrorEnvelope:
    if response.status_code == 202:
        response_202 = ClaimEdge.from_dict(response.json())

        return response_202

    if response.status_code == 415:
        response_415 = ErrorEnvelope.from_dict(response.json())

        return response_415

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
    *,
    client: AuthenticatedClient | Client,
    body: ClaimEdgeProposal,
    idempotency_key: str,
) -> Response[ClaimEdge | ErrorEnvelope]:
    """Propose a claim-to-claim connection; pending until ratified.

     Requires: dreamer(propose). The claim-edge label vocabulary is not fixed by the design (open detail,
    roadmap).

    Args:
        idempotency_key (str):
        body (ClaimEdgeProposal):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ClaimEdge | ErrorEnvelope]
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
    body: ClaimEdgeProposal,
    idempotency_key: str,
) -> ClaimEdge | ErrorEnvelope | None:
    """Propose a claim-to-claim connection; pending until ratified.

     Requires: dreamer(propose). The claim-edge label vocabulary is not fixed by the design (open detail,
    roadmap).

    Args:
        idempotency_key (str):
        body (ClaimEdgeProposal):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ClaimEdge | ErrorEnvelope
    """

    return sync_detailed(
        client=client,
        body=body,
        idempotency_key=idempotency_key,
    ).parsed


async def asyncio_detailed(
    *,
    client: AuthenticatedClient | Client,
    body: ClaimEdgeProposal,
    idempotency_key: str,
) -> Response[ClaimEdge | ErrorEnvelope]:
    """Propose a claim-to-claim connection; pending until ratified.

     Requires: dreamer(propose). The claim-edge label vocabulary is not fixed by the design (open detail,
    roadmap).

    Args:
        idempotency_key (str):
        body (ClaimEdgeProposal):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ClaimEdge | ErrorEnvelope]
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
    body: ClaimEdgeProposal,
    idempotency_key: str,
) -> ClaimEdge | ErrorEnvelope | None:
    """Propose a claim-to-claim connection; pending until ratified.

     Requires: dreamer(propose). The claim-edge label vocabulary is not fixed by the design (open detail,
    roadmap).

    Args:
        idempotency_key (str):
        body (ClaimEdgeProposal):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ClaimEdge | ErrorEnvelope
    """

    return (
        await asyncio_detailed(
            client=client,
            body=body,
            idempotency_key=idempotency_key,
        )
    ).parsed
