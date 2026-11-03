from http import HTTPStatus
from typing import Any

import httpx

from ...client import AuthenticatedClient, Client
from ...models.error_envelope import ErrorEnvelope
from ...models.submit_accepted import SubmitAccepted
from ...models.submit_request import SubmitRequest
from ...types import UNSET, Response, Unset


def _get_kwargs(
    *,
    body: SubmitRequest,
    idempotency_key: str,
    x_oml_workarea: str | Unset = UNSET,
) -> dict[str, Any]:
    headers: dict[str, Any] = {}
    headers["Idempotency-Key"] = idempotency_key

    if not isinstance(x_oml_workarea, Unset):
        headers["X-OML-Workarea"] = x_oml_workarea

    _kwargs: dict[str, Any] = {
        "method": "post",
        "url": "/v1/memories",
    }

    _kwargs["json"] = body.to_dict()

    headers["Content-Type"] = "application/json"

    _kwargs["headers"] = headers
    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> ErrorEnvelope | SubmitAccepted:
    if response.status_code == 202:
        response_202 = SubmitAccepted.from_dict(response.json())

        return response_202

    if response.status_code == 409:
        response_409 = ErrorEnvelope.from_dict(response.json())

        return response_409

    if response.status_code == 415:
        response_415 = ErrorEnvelope.from_dict(response.json())

        return response_415

    if response.status_code == 422:
        response_422 = ErrorEnvelope.from_dict(response.json())

        return response_422

    if response.status_code == 429:
        response_429 = ErrorEnvelope.from_dict(response.json())

        return response_429

    response_default = ErrorEnvelope.from_dict(response.json())

    return response_default


def _build_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Response[ErrorEnvelope | SubmitAccepted]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    *,
    client: AuthenticatedClient | Client,
    body: SubmitRequest,
    idempotency_key: str,
    x_oml_workarea: str | Unset = UNSET,
) -> Response[ErrorEnvelope | SubmitAccepted]:
    """Submit a memory (remember or learn); held on arrival, scanned same-call.

     Requires: writer effective access on the resolved workarea; every derives_from ancestor must be
    readable by the key — promoted within its reader effective access, or its own row in any state —
    else 404 access.not_found, indistinguishable from a nonexistent id (ADR-0025 d3); lineage is
    attribution, not a verb of its own. The target workarea resolves from the token (default_workarea,
    or the X-OML-Workarea override; request.workarea_unresolved with neither — ADR-0022 d4). Journals
    submission.remember or submission.learn; verb learn with an empty or omitted derives_from is refused
    (submission.lineage_required).

    Args:
        idempotency_key (str):
        x_oml_workarea (str | Unset): A workarea, by its node id or its name. Resolved server-side
            before any check that reads state; an unknown name answers exactly what an unknown id
            answers. A derived path is neither, and is refused (request.validation).
        body (SubmitRequest): Names no workarea: the target resolves from the key's
            default_workarea or the X-OML-Workarea header (ADR-0022 d4); neither present is refused
            with request.workarea_unresolved. This body, and its nested objects, stay open: ADR-0025
            d16 closes the recall, supersede, key-issuance and grant bodies only.

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | SubmitAccepted]
    """

    kwargs = _get_kwargs(
        body=body,
        idempotency_key=idempotency_key,
        x_oml_workarea=x_oml_workarea,
    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    *,
    client: AuthenticatedClient | Client,
    body: SubmitRequest,
    idempotency_key: str,
    x_oml_workarea: str | Unset = UNSET,
) -> ErrorEnvelope | SubmitAccepted | None:
    """Submit a memory (remember or learn); held on arrival, scanned same-call.

     Requires: writer effective access on the resolved workarea; every derives_from ancestor must be
    readable by the key — promoted within its reader effective access, or its own row in any state —
    else 404 access.not_found, indistinguishable from a nonexistent id (ADR-0025 d3); lineage is
    attribution, not a verb of its own. The target workarea resolves from the token (default_workarea,
    or the X-OML-Workarea override; request.workarea_unresolved with neither — ADR-0022 d4). Journals
    submission.remember or submission.learn; verb learn with an empty or omitted derives_from is refused
    (submission.lineage_required).

    Args:
        idempotency_key (str):
        x_oml_workarea (str | Unset): A workarea, by its node id or its name. Resolved server-side
            before any check that reads state; an unknown name answers exactly what an unknown id
            answers. A derived path is neither, and is refused (request.validation).
        body (SubmitRequest): Names no workarea: the target resolves from the key's
            default_workarea or the X-OML-Workarea header (ADR-0022 d4); neither present is refused
            with request.workarea_unresolved. This body, and its nested objects, stay open: ADR-0025
            d16 closes the recall, supersede, key-issuance and grant bodies only.

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | SubmitAccepted
    """

    return sync_detailed(
        client=client,
        body=body,
        idempotency_key=idempotency_key,
        x_oml_workarea=x_oml_workarea,
    ).parsed


async def asyncio_detailed(
    *,
    client: AuthenticatedClient | Client,
    body: SubmitRequest,
    idempotency_key: str,
    x_oml_workarea: str | Unset = UNSET,
) -> Response[ErrorEnvelope | SubmitAccepted]:
    """Submit a memory (remember or learn); held on arrival, scanned same-call.

     Requires: writer effective access on the resolved workarea; every derives_from ancestor must be
    readable by the key — promoted within its reader effective access, or its own row in any state —
    else 404 access.not_found, indistinguishable from a nonexistent id (ADR-0025 d3); lineage is
    attribution, not a verb of its own. The target workarea resolves from the token (default_workarea,
    or the X-OML-Workarea override; request.workarea_unresolved with neither — ADR-0022 d4). Journals
    submission.remember or submission.learn; verb learn with an empty or omitted derives_from is refused
    (submission.lineage_required).

    Args:
        idempotency_key (str):
        x_oml_workarea (str | Unset): A workarea, by its node id or its name. Resolved server-side
            before any check that reads state; an unknown name answers exactly what an unknown id
            answers. A derived path is neither, and is refused (request.validation).
        body (SubmitRequest): Names no workarea: the target resolves from the key's
            default_workarea or the X-OML-Workarea header (ADR-0022 d4); neither present is refused
            with request.workarea_unresolved. This body, and its nested objects, stay open: ADR-0025
            d16 closes the recall, supersede, key-issuance and grant bodies only.

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | SubmitAccepted]
    """

    kwargs = _get_kwargs(
        body=body,
        idempotency_key=idempotency_key,
        x_oml_workarea=x_oml_workarea,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    *,
    client: AuthenticatedClient | Client,
    body: SubmitRequest,
    idempotency_key: str,
    x_oml_workarea: str | Unset = UNSET,
) -> ErrorEnvelope | SubmitAccepted | None:
    """Submit a memory (remember or learn); held on arrival, scanned same-call.

     Requires: writer effective access on the resolved workarea; every derives_from ancestor must be
    readable by the key — promoted within its reader effective access, or its own row in any state —
    else 404 access.not_found, indistinguishable from a nonexistent id (ADR-0025 d3); lineage is
    attribution, not a verb of its own. The target workarea resolves from the token (default_workarea,
    or the X-OML-Workarea override; request.workarea_unresolved with neither — ADR-0022 d4). Journals
    submission.remember or submission.learn; verb learn with an empty or omitted derives_from is refused
    (submission.lineage_required).

    Args:
        idempotency_key (str):
        x_oml_workarea (str | Unset): A workarea, by its node id or its name. Resolved server-side
            before any check that reads state; an unknown name answers exactly what an unknown id
            answers. A derived path is neither, and is refused (request.validation).
        body (SubmitRequest): Names no workarea: the target resolves from the key's
            default_workarea or the X-OML-Workarea header (ADR-0022 d4); neither present is refused
            with request.workarea_unresolved. This body, and its nested objects, stay open: ADR-0025
            d16 closes the recall, supersede, key-issuance and grant bodies only.

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | SubmitAccepted
    """

    return (
        await asyncio_detailed(
            client=client,
            body=body,
            idempotency_key=idempotency_key,
            x_oml_workarea=x_oml_workarea,
        )
    ).parsed
