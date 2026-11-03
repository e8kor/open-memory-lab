from http import HTTPStatus
from typing import Any
from urllib.parse import quote

import httpx

from ...client import AuthenticatedClient, Client
from ...models.error_envelope import ErrorEnvelope
from ...models.workarea_edge import WorkareaEdge
from ...models.workarea_edge_proposal import WorkareaEdgeProposal
from ...types import Response


def _get_kwargs(
    workarea_id: str,
    *,
    body: WorkareaEdgeProposal,
    idempotency_key: str,
) -> dict[str, Any]:
    headers: dict[str, Any] = {}
    headers["Idempotency-Key"] = idempotency_key

    _kwargs: dict[str, Any] = {
        "method": "post",
        "url": "/v2/workareas/{workarea_id}/edges".format(
            workarea_id=quote(str(workarea_id), safe=""),
        ),
    }

    _kwargs["json"] = body.to_dict()

    headers["Content-Type"] = "application/json"

    _kwargs["headers"] = headers
    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> ErrorEnvelope | WorkareaEdge:
    if response.status_code == 202:
        response_202 = WorkareaEdge.from_dict(response.json())

        return response_202

    if response.status_code == 415:
        response_415 = ErrorEnvelope.from_dict(response.json())

        return response_415

    response_default = ErrorEnvelope.from_dict(response.json())

    return response_default


def _build_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Response[ErrorEnvelope | WorkareaEdge]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    workarea_id: str,
    *,
    client: AuthenticatedClient | Client,
    body: WorkareaEdgeProposal,
    idempotency_key: str,
) -> Response[ErrorEnvelope | WorkareaEdge]:
    """Propose a labelled edge from this node; pending until ratified.

     Requires: dreamer(propose) effective access on at least one endpoint and reader on both (edge-
    authority rule, D7).

    Args:
        workarea_id (str): A workarea, by its node id or its name. Resolved server-side before any
            check that reads state; an unknown name answers exactly what an unknown id answers. A
            derived path is neither, and is refused (request.validation).
        idempotency_key (str):
        body (WorkareaEdgeProposal):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | WorkareaEdge]
    """

    kwargs = _get_kwargs(
        workarea_id=workarea_id,
        body=body,
        idempotency_key=idempotency_key,
    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    workarea_id: str,
    *,
    client: AuthenticatedClient | Client,
    body: WorkareaEdgeProposal,
    idempotency_key: str,
) -> ErrorEnvelope | WorkareaEdge | None:
    """Propose a labelled edge from this node; pending until ratified.

     Requires: dreamer(propose) effective access on at least one endpoint and reader on both (edge-
    authority rule, D7).

    Args:
        workarea_id (str): A workarea, by its node id or its name. Resolved server-side before any
            check that reads state; an unknown name answers exactly what an unknown id answers. A
            derived path is neither, and is refused (request.validation).
        idempotency_key (str):
        body (WorkareaEdgeProposal):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | WorkareaEdge
    """

    return sync_detailed(
        workarea_id=workarea_id,
        client=client,
        body=body,
        idempotency_key=idempotency_key,
    ).parsed


async def asyncio_detailed(
    workarea_id: str,
    *,
    client: AuthenticatedClient | Client,
    body: WorkareaEdgeProposal,
    idempotency_key: str,
) -> Response[ErrorEnvelope | WorkareaEdge]:
    """Propose a labelled edge from this node; pending until ratified.

     Requires: dreamer(propose) effective access on at least one endpoint and reader on both (edge-
    authority rule, D7).

    Args:
        workarea_id (str): A workarea, by its node id or its name. Resolved server-side before any
            check that reads state; an unknown name answers exactly what an unknown id answers. A
            derived path is neither, and is refused (request.validation).
        idempotency_key (str):
        body (WorkareaEdgeProposal):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | WorkareaEdge]
    """

    kwargs = _get_kwargs(
        workarea_id=workarea_id,
        body=body,
        idempotency_key=idempotency_key,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    workarea_id: str,
    *,
    client: AuthenticatedClient | Client,
    body: WorkareaEdgeProposal,
    idempotency_key: str,
) -> ErrorEnvelope | WorkareaEdge | None:
    """Propose a labelled edge from this node; pending until ratified.

     Requires: dreamer(propose) effective access on at least one endpoint and reader on both (edge-
    authority rule, D7).

    Args:
        workarea_id (str): A workarea, by its node id or its name. Resolved server-side before any
            check that reads state; an unknown name answers exactly what an unknown id answers. A
            derived path is neither, and is refused (request.validation).
        idempotency_key (str):
        body (WorkareaEdgeProposal):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | WorkareaEdge
    """

    return (
        await asyncio_detailed(
            workarea_id=workarea_id,
            client=client,
            body=body,
            idempotency_key=idempotency_key,
        )
    ).parsed
