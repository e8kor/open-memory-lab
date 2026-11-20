from http import HTTPStatus
from typing import Any

import httpx

from ...client import AuthenticatedClient, Client
from ...models.admin_open_investigation_body import AdminOpenInvestigationBody
from ...models.error_envelope import ErrorEnvelope
from ...models.investigation import Investigation
from ...types import UNSET, Response, Unset


def _get_kwargs(
    *,
    body: AdminOpenInvestigationBody,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> dict[str, Any]:
    headers: dict[str, Any] = {}
    if not isinstance(x_request_id, Unset):
        headers["X-Request-Id"] = x_request_id

    if not isinstance(x_correlation_id, Unset):
        headers["X-Correlation-Id"] = x_correlation_id

    if not isinstance(traceparent, Unset):
        headers["traceparent"] = traceparent

    _kwargs: dict[str, Any] = {
        "method": "post",
        "url": "/v2/admin/investigations",
    }

    _kwargs["json"] = body.to_dict()

    headers["Content-Type"] = "application/json"

    _kwargs["headers"] = headers
    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> ErrorEnvelope | Investigation:
    if response.status_code == 201:
        response_201 = Investigation.from_dict(response.json())

        return response_201

    if response.status_code == 403:
        response_403 = ErrorEnvelope.from_dict(response.json())

        return response_403

    response_default = ErrorEnvelope.from_dict(response.json())

    return response_default


def _build_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Response[ErrorEnvelope | Investigation]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    *,
    client: AuthenticatedClient | Client,
    body: AdminOpenInvestigationBody,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> Response[ErrorEnvelope | Investigation]:
    """Open an investigation, the reason the admin key's reads of claim text are made under.

     Requires: admin key role (ADR-0089 d10, d15); every other role answers access.verb_ceiling. reason
    is 1 to 1,024 bytes, stored in clear and scanned as a verdict's reason is; a veto answers
    submission.scan_veto. Journalled as admin.investigation_opened with the id and the reason. Not
    idempotent: each call opens one. The investigation stays open until its key closes it. Since v0.8.4.

    Args:
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        body (AdminOpenInvestigationBody):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | Investigation]
    """

    kwargs = _get_kwargs(
        body=body,
        x_request_id=x_request_id,
        x_correlation_id=x_correlation_id,
        traceparent=traceparent,
    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    *,
    client: AuthenticatedClient | Client,
    body: AdminOpenInvestigationBody,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> ErrorEnvelope | Investigation | None:
    """Open an investigation, the reason the admin key's reads of claim text are made under.

     Requires: admin key role (ADR-0089 d10, d15); every other role answers access.verb_ceiling. reason
    is 1 to 1,024 bytes, stored in clear and scanned as a verdict's reason is; a veto answers
    submission.scan_veto. Journalled as admin.investigation_opened with the id and the reason. Not
    idempotent: each call opens one. The investigation stays open until its key closes it. Since v0.8.4.

    Args:
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        body (AdminOpenInvestigationBody):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | Investigation
    """

    return sync_detailed(
        client=client,
        body=body,
        x_request_id=x_request_id,
        x_correlation_id=x_correlation_id,
        traceparent=traceparent,
    ).parsed


async def asyncio_detailed(
    *,
    client: AuthenticatedClient | Client,
    body: AdminOpenInvestigationBody,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> Response[ErrorEnvelope | Investigation]:
    """Open an investigation, the reason the admin key's reads of claim text are made under.

     Requires: admin key role (ADR-0089 d10, d15); every other role answers access.verb_ceiling. reason
    is 1 to 1,024 bytes, stored in clear and scanned as a verdict's reason is; a veto answers
    submission.scan_veto. Journalled as admin.investigation_opened with the id and the reason. Not
    idempotent: each call opens one. The investigation stays open until its key closes it. Since v0.8.4.

    Args:
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        body (AdminOpenInvestigationBody):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | Investigation]
    """

    kwargs = _get_kwargs(
        body=body,
        x_request_id=x_request_id,
        x_correlation_id=x_correlation_id,
        traceparent=traceparent,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    *,
    client: AuthenticatedClient | Client,
    body: AdminOpenInvestigationBody,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> ErrorEnvelope | Investigation | None:
    """Open an investigation, the reason the admin key's reads of claim text are made under.

     Requires: admin key role (ADR-0089 d10, d15); every other role answers access.verb_ceiling. reason
    is 1 to 1,024 bytes, stored in clear and scanned as a verdict's reason is; a veto answers
    submission.scan_veto. Journalled as admin.investigation_opened with the id and the reason. Not
    idempotent: each call opens one. The investigation stays open until its key closes it. Since v0.8.4.

    Args:
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        body (AdminOpenInvestigationBody):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | Investigation
    """

    return (
        await asyncio_detailed(
            client=client,
            body=body,
            x_request_id=x_request_id,
            x_correlation_id=x_correlation_id,
            traceparent=traceparent,
        )
    ).parsed
