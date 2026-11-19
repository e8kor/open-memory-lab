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
        "url": "/v2/review/fixes/{fix_id}/confirm".format(
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
    """Confirm an admin fix; the version it serves reads approver.confirmed true from then on.

     Requires: a person's reviewer key with reviewer on the fix's workspace (ADR-0089 d8, d15). A key of
    the admin's own person confirms, marked same_user, only a fix whose target lies inside that person's
    own root, in a workspace neither protected nor frozen, and is a member of no release; anywhere else
    it answers review.author_in_lineage with details.reason same_person (amendment of 2026-11-18). A fix
    already settled answers admin.fix_settled. Journalled as admin.fix_confirmed. Since v0.8.4.

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
    """Confirm an admin fix; the version it serves reads approver.confirmed true from then on.

     Requires: a person's reviewer key with reviewer on the fix's workspace (ADR-0089 d8, d15). A key of
    the admin's own person confirms, marked same_user, only a fix whose target lies inside that person's
    own root, in a workspace neither protected nor frozen, and is a member of no release; anywhere else
    it answers review.author_in_lineage with details.reason same_person (amendment of 2026-11-18). A fix
    already settled answers admin.fix_settled. Journalled as admin.fix_confirmed. Since v0.8.4.

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
    """Confirm an admin fix; the version it serves reads approver.confirmed true from then on.

     Requires: a person's reviewer key with reviewer on the fix's workspace (ADR-0089 d8, d15). A key of
    the admin's own person confirms, marked same_user, only a fix whose target lies inside that person's
    own root, in a workspace neither protected nor frozen, and is a member of no release; anywhere else
    it answers review.author_in_lineage with details.reason same_person (amendment of 2026-11-18). A fix
    already settled answers admin.fix_settled. Journalled as admin.fix_confirmed. Since v0.8.4.

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
    """Confirm an admin fix; the version it serves reads approver.confirmed true from then on.

     Requires: a person's reviewer key with reviewer on the fix's workspace (ADR-0089 d8, d15). A key of
    the admin's own person confirms, marked same_user, only a fix whose target lies inside that person's
    own root, in a workspace neither protected nor frozen, and is a member of no release; anywhere else
    it answers review.author_in_lineage with details.reason same_person (amendment of 2026-11-18). A fix
    already settled answers admin.fix_settled. Journalled as admin.fix_confirmed. Since v0.8.4.

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
