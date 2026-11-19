from http import HTTPStatus
from typing import Any
from urllib.parse import quote
from uuid import UUID

import httpx

from ...client import AuthenticatedClient, Client
from ...models.error_envelope import ErrorEnvelope
from ...models.fix import Fix
from ...types import UNSET, Response, Unset


def _get_kwargs(
    fix_id: UUID,
    *,
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
        "url": "/v2/review/fixes/{fix_id}/revert".format(
            fix_id=quote(str(fix_id), safe=""),
        ),
    }

    _kwargs["headers"] = headers
    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> ErrorEnvelope | Fix:
    if response.status_code == 200:
        response_200 = Fix.from_dict(response.json())

        return response_200

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
    fix_id: UUID,
    *,
    client: AuthenticatedClient | Client,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> Response[ErrorEnvelope | Fix]:
    """Revert an admin fix; the text it took out of service returns as a new version this key approves.

     Requires: as confirm (ADR-0089 d8, d15). The restored version is authored by the tenant's reserved
    server key (principal oml:server, used_by service, revoked at issuance), derives from the version
    the fix took out of service, and is promoted by this key's verdict; R2 excludes that version's
    lineage and the admin's principal. An erasure answers admin.fix_not_revertible; a fix whose claim
    moved on admin.fix_superseded; a frozen workspace workspace.frozen. Journalled as
    admin.fix_reverted, after the submission, verdict and promotion it causes. Since v0.8.3.

    Args:
        fix_id (UUID):
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | Fix]
    """

    kwargs = _get_kwargs(
        fix_id=fix_id,
        x_request_id=x_request_id,
        x_correlation_id=x_correlation_id,
        traceparent=traceparent,
    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    fix_id: UUID,
    *,
    client: AuthenticatedClient | Client,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> ErrorEnvelope | Fix | None:
    """Revert an admin fix; the text it took out of service returns as a new version this key approves.

     Requires: as confirm (ADR-0089 d8, d15). The restored version is authored by the tenant's reserved
    server key (principal oml:server, used_by service, revoked at issuance), derives from the version
    the fix took out of service, and is promoted by this key's verdict; R2 excludes that version's
    lineage and the admin's principal. An erasure answers admin.fix_not_revertible; a fix whose claim
    moved on admin.fix_superseded; a frozen workspace workspace.frozen. Journalled as
    admin.fix_reverted, after the submission, verdict and promotion it causes. Since v0.8.3.

    Args:
        fix_id (UUID):
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | Fix
    """

    return sync_detailed(
        fix_id=fix_id,
        client=client,
        x_request_id=x_request_id,
        x_correlation_id=x_correlation_id,
        traceparent=traceparent,
    ).parsed


async def asyncio_detailed(
    fix_id: UUID,
    *,
    client: AuthenticatedClient | Client,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> Response[ErrorEnvelope | Fix]:
    """Revert an admin fix; the text it took out of service returns as a new version this key approves.

     Requires: as confirm (ADR-0089 d8, d15). The restored version is authored by the tenant's reserved
    server key (principal oml:server, used_by service, revoked at issuance), derives from the version
    the fix took out of service, and is promoted by this key's verdict; R2 excludes that version's
    lineage and the admin's principal. An erasure answers admin.fix_not_revertible; a fix whose claim
    moved on admin.fix_superseded; a frozen workspace workspace.frozen. Journalled as
    admin.fix_reverted, after the submission, verdict and promotion it causes. Since v0.8.3.

    Args:
        fix_id (UUID):
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | Fix]
    """

    kwargs = _get_kwargs(
        fix_id=fix_id,
        x_request_id=x_request_id,
        x_correlation_id=x_correlation_id,
        traceparent=traceparent,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    fix_id: UUID,
    *,
    client: AuthenticatedClient | Client,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> ErrorEnvelope | Fix | None:
    """Revert an admin fix; the text it took out of service returns as a new version this key approves.

     Requires: as confirm (ADR-0089 d8, d15). The restored version is authored by the tenant's reserved
    server key (principal oml:server, used_by service, revoked at issuance), derives from the version
    the fix took out of service, and is promoted by this key's verdict; R2 excludes that version's
    lineage and the admin's principal. An erasure answers admin.fix_not_revertible; a fix whose claim
    moved on admin.fix_superseded; a frozen workspace workspace.frozen. Journalled as
    admin.fix_reverted, after the submission, verdict and promotion it causes. Since v0.8.3.

    Args:
        fix_id (UUID):
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | Fix
    """

    return (
        await asyncio_detailed(
            fix_id=fix_id,
            client=client,
            x_request_id=x_request_id,
            x_correlation_id=x_correlation_id,
            traceparent=traceparent,
        )
    ).parsed
