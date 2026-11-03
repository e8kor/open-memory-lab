from http import HTTPStatus
from typing import Any
from urllib.parse import quote

import httpx

from ...client import AuthenticatedClient, Client
from ...models.error_envelope import ErrorEnvelope
from ...models.workarea_crew_update import WorkareaCrewUpdate
from ...types import Response


def _get_kwargs(
    workarea_id: str,
    *,
    body: WorkareaCrewUpdate,
    idempotency_key: str,
) -> dict[str, Any]:
    headers: dict[str, Any] = {}
    headers["Idempotency-Key"] = idempotency_key

    _kwargs: dict[str, Any] = {
        "method": "post",
        "url": "/v2/workareas/{workarea_id}/crew".format(
            workarea_id=quote(str(workarea_id), safe=""),
        ),
    }

    _kwargs["json"] = body.to_dict()

    headers["Content-Type"] = "application/json"

    _kwargs["headers"] = headers
    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> ErrorEnvelope:
    if response.status_code == 415:
        response_415 = ErrorEnvelope.from_dict(response.json())

        return response_415

    response_default = ErrorEnvelope.from_dict(response.json())

    return response_default


def _build_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Response[ErrorEnvelope]:
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
    body: WorkareaCrewUpdate,
    idempotency_key: str,
) -> Response[ErrorEnvelope]:
    """Journalled crew-metadata update — points the workarea at a crew record, by id or name; allowed on
    validated areas (freeze covers claims, not metadata).

     Requires: admin key role. Journals workarea.crew_updated with the crew's id (ADR-0026 d8); a crew
    that names no crew record of the tenant answers access.not_found, checked after the workarea. The
    crew record itself is updated at POST /v2/crews/{crew_id}/update.

    Args:
        workarea_id (str): A workarea, by its node id or its name. Resolved server-side before any
            check that reads state; an unknown name answers exactly what an unknown id answers. A
            derived path is neither, and is refused (request.validation).
        idempotency_key (str):
        body (WorkareaCrewUpdate):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope]
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
    body: WorkareaCrewUpdate,
    idempotency_key: str,
) -> ErrorEnvelope | None:
    """Journalled crew-metadata update — points the workarea at a crew record, by id or name; allowed on
    validated areas (freeze covers claims, not metadata).

     Requires: admin key role. Journals workarea.crew_updated with the crew's id (ADR-0026 d8); a crew
    that names no crew record of the tenant answers access.not_found, checked after the workarea. The
    crew record itself is updated at POST /v2/crews/{crew_id}/update.

    Args:
        workarea_id (str): A workarea, by its node id or its name. Resolved server-side before any
            check that reads state; an unknown name answers exactly what an unknown id answers. A
            derived path is neither, and is refused (request.validation).
        idempotency_key (str):
        body (WorkareaCrewUpdate):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope
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
    body: WorkareaCrewUpdate,
    idempotency_key: str,
) -> Response[ErrorEnvelope]:
    """Journalled crew-metadata update — points the workarea at a crew record, by id or name; allowed on
    validated areas (freeze covers claims, not metadata).

     Requires: admin key role. Journals workarea.crew_updated with the crew's id (ADR-0026 d8); a crew
    that names no crew record of the tenant answers access.not_found, checked after the workarea. The
    crew record itself is updated at POST /v2/crews/{crew_id}/update.

    Args:
        workarea_id (str): A workarea, by its node id or its name. Resolved server-side before any
            check that reads state; an unknown name answers exactly what an unknown id answers. A
            derived path is neither, and is refused (request.validation).
        idempotency_key (str):
        body (WorkareaCrewUpdate):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope]
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
    body: WorkareaCrewUpdate,
    idempotency_key: str,
) -> ErrorEnvelope | None:
    """Journalled crew-metadata update — points the workarea at a crew record, by id or name; allowed on
    validated areas (freeze covers claims, not metadata).

     Requires: admin key role. Journals workarea.crew_updated with the crew's id (ADR-0026 d8); a crew
    that names no crew record of the tenant answers access.not_found, checked after the workarea. The
    crew record itself is updated at POST /v2/crews/{crew_id}/update.

    Args:
        workarea_id (str): A workarea, by its node id or its name. Resolved server-side before any
            check that reads state; an unknown name answers exactly what an unknown id answers. A
            derived path is neither, and is refused (request.validation).
        idempotency_key (str):
        body (WorkareaCrewUpdate):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope
    """

    return (
        await asyncio_detailed(
            workarea_id=workarea_id,
            client=client,
            body=body,
            idempotency_key=idempotency_key,
        )
    ).parsed
