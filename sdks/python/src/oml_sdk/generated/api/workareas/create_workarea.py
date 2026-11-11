from http import HTTPStatus
from typing import Any

import httpx

from ...client import AuthenticatedClient, Client
from ...models.error_envelope import ErrorEnvelope
from ...models.workarea import Workarea
from ...models.workarea_create import WorkareaCreate
from ...types import UNSET, Response, Unset


def _get_kwargs(
    *,
    body: WorkareaCreate,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
    idempotency_key: str,
) -> dict[str, Any]:
    headers: dict[str, Any] = {}
    if not isinstance(x_request_id, Unset):
        headers["X-Request-Id"] = x_request_id

    if not isinstance(x_correlation_id, Unset):
        headers["X-Correlation-Id"] = x_correlation_id

    if not isinstance(traceparent, Unset):
        headers["traceparent"] = traceparent

    headers["Idempotency-Key"] = idempotency_key

    _kwargs: dict[str, Any] = {
        "method": "post",
        "url": "/v2/workareas",
    }

    _kwargs["json"] = body.to_dict()

    headers["Content-Type"] = "application/json"

    _kwargs["headers"] = headers
    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> ErrorEnvelope | Workarea:
    if response.status_code == 201:
        response_201 = Workarea.from_dict(response.json())

        return response_201

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
) -> Response[ErrorEnvelope | Workarea]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    *,
    client: AuthenticatedClient | Client,
    body: WorkareaCreate,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
    idempotency_key: str,
) -> Response[ErrorEnvelope | Workarea]:
    """Create a child workarea in development.

     Requires: admin key role. parent and crew each take an id or a name, and parent_id, the v0.3.0
    spelling, is refused with request.validation, details.renamed_to /parent (ADR-0039 d7); a crew that
    names no crew record of the tenant answers access.not_found, checked after the parent and before
    name.taken. name must be unique in the tenant (409 name.taken, checked after every other check;
    universe is always taken, by the root). Refused under a validated parent (workarea.frozen); universe
    cannot be created or deleted. A resubmission under the same Idempotency-Key and body answers the
    recorded creation with Idempotency-Replay: true and creates nothing again; the same key with another
    body is idempotency.digest_mismatch (ADR-0070 d3 layer 4; since v0.7.0). A person's web session key
    (<user>-web, through its capacity credential) creates under its own root, as the web console's
    Workareas view does (ADR-0070 d5).

    Args:
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        idempotency_key (str):
        body (WorkareaCreate):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | Workarea]
    """

    kwargs = _get_kwargs(
        body=body,
        x_request_id=x_request_id,
        x_correlation_id=x_correlation_id,
        traceparent=traceparent,
        idempotency_key=idempotency_key,
    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    *,
    client: AuthenticatedClient | Client,
    body: WorkareaCreate,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
    idempotency_key: str,
) -> ErrorEnvelope | Workarea | None:
    """Create a child workarea in development.

     Requires: admin key role. parent and crew each take an id or a name, and parent_id, the v0.3.0
    spelling, is refused with request.validation, details.renamed_to /parent (ADR-0039 d7); a crew that
    names no crew record of the tenant answers access.not_found, checked after the parent and before
    name.taken. name must be unique in the tenant (409 name.taken, checked after every other check;
    universe is always taken, by the root). Refused under a validated parent (workarea.frozen); universe
    cannot be created or deleted. A resubmission under the same Idempotency-Key and body answers the
    recorded creation with Idempotency-Replay: true and creates nothing again; the same key with another
    body is idempotency.digest_mismatch (ADR-0070 d3 layer 4; since v0.7.0). A person's web session key
    (<user>-web, through its capacity credential) creates under its own root, as the web console's
    Workareas view does (ADR-0070 d5).

    Args:
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        idempotency_key (str):
        body (WorkareaCreate):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | Workarea
    """

    return sync_detailed(
        client=client,
        body=body,
        x_request_id=x_request_id,
        x_correlation_id=x_correlation_id,
        traceparent=traceparent,
        idempotency_key=idempotency_key,
    ).parsed


async def asyncio_detailed(
    *,
    client: AuthenticatedClient | Client,
    body: WorkareaCreate,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
    idempotency_key: str,
) -> Response[ErrorEnvelope | Workarea]:
    """Create a child workarea in development.

     Requires: admin key role. parent and crew each take an id or a name, and parent_id, the v0.3.0
    spelling, is refused with request.validation, details.renamed_to /parent (ADR-0039 d7); a crew that
    names no crew record of the tenant answers access.not_found, checked after the parent and before
    name.taken. name must be unique in the tenant (409 name.taken, checked after every other check;
    universe is always taken, by the root). Refused under a validated parent (workarea.frozen); universe
    cannot be created or deleted. A resubmission under the same Idempotency-Key and body answers the
    recorded creation with Idempotency-Replay: true and creates nothing again; the same key with another
    body is idempotency.digest_mismatch (ADR-0070 d3 layer 4; since v0.7.0). A person's web session key
    (<user>-web, through its capacity credential) creates under its own root, as the web console's
    Workareas view does (ADR-0070 d5).

    Args:
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        idempotency_key (str):
        body (WorkareaCreate):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | Workarea]
    """

    kwargs = _get_kwargs(
        body=body,
        x_request_id=x_request_id,
        x_correlation_id=x_correlation_id,
        traceparent=traceparent,
        idempotency_key=idempotency_key,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    *,
    client: AuthenticatedClient | Client,
    body: WorkareaCreate,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
    idempotency_key: str,
) -> ErrorEnvelope | Workarea | None:
    """Create a child workarea in development.

     Requires: admin key role. parent and crew each take an id or a name, and parent_id, the v0.3.0
    spelling, is refused with request.validation, details.renamed_to /parent (ADR-0039 d7); a crew that
    names no crew record of the tenant answers access.not_found, checked after the parent and before
    name.taken. name must be unique in the tenant (409 name.taken, checked after every other check;
    universe is always taken, by the root). Refused under a validated parent (workarea.frozen); universe
    cannot be created or deleted. A resubmission under the same Idempotency-Key and body answers the
    recorded creation with Idempotency-Replay: true and creates nothing again; the same key with another
    body is idempotency.digest_mismatch (ADR-0070 d3 layer 4; since v0.7.0). A person's web session key
    (<user>-web, through its capacity credential) creates under its own root, as the web console's
    Workareas view does (ADR-0070 d5).

    Args:
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        idempotency_key (str):
        body (WorkareaCreate):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | Workarea
    """

    return (
        await asyncio_detailed(
            client=client,
            body=body,
            x_request_id=x_request_id,
            x_correlation_id=x_correlation_id,
            traceparent=traceparent,
            idempotency_key=idempotency_key,
        )
    ).parsed
