from http import HTTPStatus
from typing import Any

import httpx

from ...client import AuthenticatedClient, Client
from ...models.admin_fix_body import AdminFixBody
from ...models.error_envelope import ErrorEnvelope
from ...models.fix import Fix
from ...types import UNSET, Response, Unset


def _get_kwargs(
    *,
    body: AdminFixBody,
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
        "url": "/v2/admin/fixes",
    }

    _kwargs["json"] = body.to_dict()

    headers["Content-Type"] = "application/json"

    _kwargs["headers"] = headers
    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> ErrorEnvelope | Fix:
    if response.status_code == 201:
        response_201 = Fix.from_dict(response.json())

        return response_201

    if response.status_code == 403:
        response_403 = ErrorEnvelope.from_dict(response.json())

        return response_403

    if response.status_code == 404:
        response_404 = ErrorEnvelope.from_dict(response.json())

        return response_404

    if response.status_code == 409:
        response_409 = ErrorEnvelope.from_dict(response.json())

        return response_409

    response_default = ErrorEnvelope.from_dict(response.json())

    return response_default


def _build_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Response[ErrorEnvelope | Fix]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    *,
    client: AuthenticatedClient | Client,
    body: AdminFixBody,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> Response[ErrorEnvelope | Fix]:
    """Take a marked break-glass fix that serves at once and waits for a person to confirm or revert it.

     Requires: admin key role, used by a person and linked to that person (ADR-0089 d1, the operator's
    decision of 2026-11-19); another admin key answers admin.fix_requires_person, details.reason
    not_used_by_person or unlinked, every other role access.verb_ceiling. act is demote (an approved
    version; the claim's tip returns to its latest earlier approved version, or none), edit (body and
    optional title replace the served tip), retire (the served tip is retired, reason as its body) or
    erase (every version of the claim is crypto-shredded). reason is 1 to 1,024 bytes, stored in clear
    and scanned. One commit journals the lifecycle event the act causes, then admin.fix naming it. An
    edit's body is scanned as its kind requires: a veto answers submission.scan_veto and a hold
    admin.fix_held_by_scan. In a frozen workspace an edit or retirement answers workspace.frozen. A
    demotion of a version that is not approved answers admin.fix_target_not_promoted. Not idempotent.
    Since v0.8.4.

    Args:
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        body (AdminFixBody):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | Fix]
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
    body: AdminFixBody,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> ErrorEnvelope | Fix | None:
    """Take a marked break-glass fix that serves at once and waits for a person to confirm or revert it.

     Requires: admin key role, used by a person and linked to that person (ADR-0089 d1, the operator's
    decision of 2026-11-19); another admin key answers admin.fix_requires_person, details.reason
    not_used_by_person or unlinked, every other role access.verb_ceiling. act is demote (an approved
    version; the claim's tip returns to its latest earlier approved version, or none), edit (body and
    optional title replace the served tip), retire (the served tip is retired, reason as its body) or
    erase (every version of the claim is crypto-shredded). reason is 1 to 1,024 bytes, stored in clear
    and scanned. One commit journals the lifecycle event the act causes, then admin.fix naming it. An
    edit's body is scanned as its kind requires: a veto answers submission.scan_veto and a hold
    admin.fix_held_by_scan. In a frozen workspace an edit or retirement answers workspace.frozen. A
    demotion of a version that is not approved answers admin.fix_target_not_promoted. Not idempotent.
    Since v0.8.4.

    Args:
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        body (AdminFixBody):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | Fix
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
    body: AdminFixBody,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> Response[ErrorEnvelope | Fix]:
    """Take a marked break-glass fix that serves at once and waits for a person to confirm or revert it.

     Requires: admin key role, used by a person and linked to that person (ADR-0089 d1, the operator's
    decision of 2026-11-19); another admin key answers admin.fix_requires_person, details.reason
    not_used_by_person or unlinked, every other role access.verb_ceiling. act is demote (an approved
    version; the claim's tip returns to its latest earlier approved version, or none), edit (body and
    optional title replace the served tip), retire (the served tip is retired, reason as its body) or
    erase (every version of the claim is crypto-shredded). reason is 1 to 1,024 bytes, stored in clear
    and scanned. One commit journals the lifecycle event the act causes, then admin.fix naming it. An
    edit's body is scanned as its kind requires: a veto answers submission.scan_veto and a hold
    admin.fix_held_by_scan. In a frozen workspace an edit or retirement answers workspace.frozen. A
    demotion of a version that is not approved answers admin.fix_target_not_promoted. Not idempotent.
    Since v0.8.4.

    Args:
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        body (AdminFixBody):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | Fix]
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
    body: AdminFixBody,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> ErrorEnvelope | Fix | None:
    """Take a marked break-glass fix that serves at once and waits for a person to confirm or revert it.

     Requires: admin key role, used by a person and linked to that person (ADR-0089 d1, the operator's
    decision of 2026-11-19); another admin key answers admin.fix_requires_person, details.reason
    not_used_by_person or unlinked, every other role access.verb_ceiling. act is demote (an approved
    version; the claim's tip returns to its latest earlier approved version, or none), edit (body and
    optional title replace the served tip), retire (the served tip is retired, reason as its body) or
    erase (every version of the claim is crypto-shredded). reason is 1 to 1,024 bytes, stored in clear
    and scanned. One commit journals the lifecycle event the act causes, then admin.fix naming it. An
    edit's body is scanned as its kind requires: a veto answers submission.scan_veto and a hold
    admin.fix_held_by_scan. In a frozen workspace an edit or retirement answers workspace.frozen. A
    demotion of a version that is not approved answers admin.fix_target_not_promoted. Not idempotent.
    Since v0.8.4.

    Args:
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        body (AdminFixBody):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | Fix
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
