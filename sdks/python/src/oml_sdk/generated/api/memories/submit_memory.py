from http import HTTPStatus
from typing import Any

import httpx

from ...client import AuthenticatedClient, Client
from ...models.error_envelope import ErrorEnvelope
from ...models.submit_accepted import SubmitAccepted
from ...models.submit_request import SubmitRequest
from ...types import Response


def _get_kwargs(
    *,
    body: SubmitRequest,
    idempotency_key: str,
) -> dict[str, Any]:
    headers: dict[str, Any] = {}
    headers["Idempotency-Key"] = idempotency_key

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
) -> Response[ErrorEnvelope | SubmitAccepted]:
    """Submit a memory (remember or learn); held on arrival, scanned same-call.

     Requires: writer effective access on the resolved workarea; every derives_from ancestor must be
    readable by the key — promoted within its reader effective access, or its own row in any state —
    else 404 access.not_found, indistinguishable from a nonexistent id (ADR-0025 d3); lineage is
    attribution, not a verb of its own. The target workarea is the body's workarea, by id or name,
    honoured only within the key's grants, else the key's default_workarea; request.workarea_unresolved
    with neither, and an X-OML-Workarea header is request.validation naming /workarea in
    details.renamed_to (ADR-0039 d7). Journals submission.remember or submission.learn; verb learn with
    an empty or omitted derives_from is refused (submission.lineage_required).

    Args:
        idempotency_key (str):
        body (SubmitRequest): The target is workarea, else the key's default_workarea; neither
            present is refused with request.workarea_unresolved (ADR-0039 d4, d7). This body, and its
            nested objects, stay open: ADR-0025 d16 closes the recall, supersede, key-issuance and
            grant bodies only. The designed attachment, simulation and vocabulary fields are declared
            under x-oml-designed-fields in openapi-designed.yaml; each sent here answers its v0.3.0
            code (ADR-0039 d9).

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | SubmitAccepted]
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
    body: SubmitRequest,
    idempotency_key: str,
) -> ErrorEnvelope | SubmitAccepted | None:
    """Submit a memory (remember or learn); held on arrival, scanned same-call.

     Requires: writer effective access on the resolved workarea; every derives_from ancestor must be
    readable by the key — promoted within its reader effective access, or its own row in any state —
    else 404 access.not_found, indistinguishable from a nonexistent id (ADR-0025 d3); lineage is
    attribution, not a verb of its own. The target workarea is the body's workarea, by id or name,
    honoured only within the key's grants, else the key's default_workarea; request.workarea_unresolved
    with neither, and an X-OML-Workarea header is request.validation naming /workarea in
    details.renamed_to (ADR-0039 d7). Journals submission.remember or submission.learn; verb learn with
    an empty or omitted derives_from is refused (submission.lineage_required).

    Args:
        idempotency_key (str):
        body (SubmitRequest): The target is workarea, else the key's default_workarea; neither
            present is refused with request.workarea_unresolved (ADR-0039 d4, d7). This body, and its
            nested objects, stay open: ADR-0025 d16 closes the recall, supersede, key-issuance and
            grant bodies only. The designed attachment, simulation and vocabulary fields are declared
            under x-oml-designed-fields in openapi-designed.yaml; each sent here answers its v0.3.0
            code (ADR-0039 d9).

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
    ).parsed


async def asyncio_detailed(
    *,
    client: AuthenticatedClient | Client,
    body: SubmitRequest,
    idempotency_key: str,
) -> Response[ErrorEnvelope | SubmitAccepted]:
    """Submit a memory (remember or learn); held on arrival, scanned same-call.

     Requires: writer effective access on the resolved workarea; every derives_from ancestor must be
    readable by the key — promoted within its reader effective access, or its own row in any state —
    else 404 access.not_found, indistinguishable from a nonexistent id (ADR-0025 d3); lineage is
    attribution, not a verb of its own. The target workarea is the body's workarea, by id or name,
    honoured only within the key's grants, else the key's default_workarea; request.workarea_unresolved
    with neither, and an X-OML-Workarea header is request.validation naming /workarea in
    details.renamed_to (ADR-0039 d7). Journals submission.remember or submission.learn; verb learn with
    an empty or omitted derives_from is refused (submission.lineage_required).

    Args:
        idempotency_key (str):
        body (SubmitRequest): The target is workarea, else the key's default_workarea; neither
            present is refused with request.workarea_unresolved (ADR-0039 d4, d7). This body, and its
            nested objects, stay open: ADR-0025 d16 closes the recall, supersede, key-issuance and
            grant bodies only. The designed attachment, simulation and vocabulary fields are declared
            under x-oml-designed-fields in openapi-designed.yaml; each sent here answers its v0.3.0
            code (ADR-0039 d9).

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | SubmitAccepted]
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
    body: SubmitRequest,
    idempotency_key: str,
) -> ErrorEnvelope | SubmitAccepted | None:
    """Submit a memory (remember or learn); held on arrival, scanned same-call.

     Requires: writer effective access on the resolved workarea; every derives_from ancestor must be
    readable by the key — promoted within its reader effective access, or its own row in any state —
    else 404 access.not_found, indistinguishable from a nonexistent id (ADR-0025 d3); lineage is
    attribution, not a verb of its own. The target workarea is the body's workarea, by id or name,
    honoured only within the key's grants, else the key's default_workarea; request.workarea_unresolved
    with neither, and an X-OML-Workarea header is request.validation naming /workarea in
    details.renamed_to (ADR-0039 d7). Journals submission.remember or submission.learn; verb learn with
    an empty or omitted derives_from is refused (submission.lineage_required).

    Args:
        idempotency_key (str):
        body (SubmitRequest): The target is workarea, else the key's default_workarea; neither
            present is refused with request.workarea_unresolved (ADR-0039 d4, d7). This body, and its
            nested objects, stay open: ADR-0025 d16 closes the recall, supersede, key-issuance and
            grant bodies only. The designed attachment, simulation and vocabulary fields are declared
            under x-oml-designed-fields in openapi-designed.yaml; each sent here answers its v0.3.0
            code (ADR-0039 d9).

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
        )
    ).parsed
