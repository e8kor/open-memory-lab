from http import HTTPStatus
from typing import Any
from urllib.parse import quote
from uuid import UUID

import httpx

from ...client import AuthenticatedClient, Client
from ...models.admin_act import AdminAct
from ...models.confirm_admin_act_body import ConfirmAdminActBody
from ...models.error_envelope import ErrorEnvelope
from ...types import UNSET, Response, Unset


def _get_kwargs(
    act: UUID,
    *,
    body: ConfirmAdminActBody,
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
        "url": "/v2/admin/acts/{act}/confirm".format(
            act=quote(str(act), safe=""),
        ),
    }

    _kwargs["json"] = body.to_dict()

    headers["Content-Type"] = "application/json"

    _kwargs["headers"] = headers
    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> AdminAct | ErrorEnvelope:
    if response.status_code == 200:
        response_200 = AdminAct.from_dict(response.json())

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
) -> Response[AdminAct | ErrorEnvelope]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    act: UUID,
    *,
    client: AuthenticatedClient | Client,
    body: ConfirmAdminActBody,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> Response[AdminAct | ErrorEnvelope]:
    """A second admin person confirms a pending widening act, which takes effect in this commit.

     Requires: admin key role, a key that counts as a live admin person of the tenant: a <user>-admin
    session key, or an admin key linked to its person (ADR-0097 d5, d11). The act's effect applies in
    the confirmation's commit: a pending key's grants and connect code, a grant, a raised limit. Refused
    with request.validation, details.field act and details.reason: not_pending (the act took effect or
    was settled), expired (pending seven days; a late confirm never revives it), own_act (the key counts
    as the person who took the act), not_admin_person (the key counts as no live admin person) or
    too_new (that person's admin flag was accepted less than seven days before the act). reason is
    required and scanned, and the act keeps it as confirm_reason; a confirmation without one is refused
    request.validation with details.field /reason and details.reason required (ADR-0097 d26; amendment
    of 2026-11-27, answer 18). An act the tenant does not hold is access.not_found. Journalled as
    admin.act with state confirmed, confirmed_by_person and confirm_reason. Since v0.8.6.

    Args:
        act (UUID):
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        body (ConfirmAdminActBody):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[AdminAct | ErrorEnvelope]
    """

    kwargs = _get_kwargs(
        act=act,
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
    act: UUID,
    *,
    client: AuthenticatedClient | Client,
    body: ConfirmAdminActBody,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> AdminAct | ErrorEnvelope | None:
    """A second admin person confirms a pending widening act, which takes effect in this commit.

     Requires: admin key role, a key that counts as a live admin person of the tenant: a <user>-admin
    session key, or an admin key linked to its person (ADR-0097 d5, d11). The act's effect applies in
    the confirmation's commit: a pending key's grants and connect code, a grant, a raised limit. Refused
    with request.validation, details.field act and details.reason: not_pending (the act took effect or
    was settled), expired (pending seven days; a late confirm never revives it), own_act (the key counts
    as the person who took the act), not_admin_person (the key counts as no live admin person) or
    too_new (that person's admin flag was accepted less than seven days before the act). reason is
    required and scanned, and the act keeps it as confirm_reason; a confirmation without one is refused
    request.validation with details.field /reason and details.reason required (ADR-0097 d26; amendment
    of 2026-11-27, answer 18). An act the tenant does not hold is access.not_found. Journalled as
    admin.act with state confirmed, confirmed_by_person and confirm_reason. Since v0.8.6.

    Args:
        act (UUID):
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        body (ConfirmAdminActBody):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        AdminAct | ErrorEnvelope
    """

    return sync_detailed(
        act=act,
        client=client,
        body=body,
        x_request_id=x_request_id,
        x_correlation_id=x_correlation_id,
        traceparent=traceparent,
    ).parsed


async def asyncio_detailed(
    act: UUID,
    *,
    client: AuthenticatedClient | Client,
    body: ConfirmAdminActBody,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> Response[AdminAct | ErrorEnvelope]:
    """A second admin person confirms a pending widening act, which takes effect in this commit.

     Requires: admin key role, a key that counts as a live admin person of the tenant: a <user>-admin
    session key, or an admin key linked to its person (ADR-0097 d5, d11). The act's effect applies in
    the confirmation's commit: a pending key's grants and connect code, a grant, a raised limit. Refused
    with request.validation, details.field act and details.reason: not_pending (the act took effect or
    was settled), expired (pending seven days; a late confirm never revives it), own_act (the key counts
    as the person who took the act), not_admin_person (the key counts as no live admin person) or
    too_new (that person's admin flag was accepted less than seven days before the act). reason is
    required and scanned, and the act keeps it as confirm_reason; a confirmation without one is refused
    request.validation with details.field /reason and details.reason required (ADR-0097 d26; amendment
    of 2026-11-27, answer 18). An act the tenant does not hold is access.not_found. Journalled as
    admin.act with state confirmed, confirmed_by_person and confirm_reason. Since v0.8.6.

    Args:
        act (UUID):
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        body (ConfirmAdminActBody):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[AdminAct | ErrorEnvelope]
    """

    kwargs = _get_kwargs(
        act=act,
        body=body,
        x_request_id=x_request_id,
        x_correlation_id=x_correlation_id,
        traceparent=traceparent,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    act: UUID,
    *,
    client: AuthenticatedClient | Client,
    body: ConfirmAdminActBody,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> AdminAct | ErrorEnvelope | None:
    """A second admin person confirms a pending widening act, which takes effect in this commit.

     Requires: admin key role, a key that counts as a live admin person of the tenant: a <user>-admin
    session key, or an admin key linked to its person (ADR-0097 d5, d11). The act's effect applies in
    the confirmation's commit: a pending key's grants and connect code, a grant, a raised limit. Refused
    with request.validation, details.field act and details.reason: not_pending (the act took effect or
    was settled), expired (pending seven days; a late confirm never revives it), own_act (the key counts
    as the person who took the act), not_admin_person (the key counts as no live admin person) or
    too_new (that person's admin flag was accepted less than seven days before the act). reason is
    required and scanned, and the act keeps it as confirm_reason; a confirmation without one is refused
    request.validation with details.field /reason and details.reason required (ADR-0097 d26; amendment
    of 2026-11-27, answer 18). An act the tenant does not hold is access.not_found. Journalled as
    admin.act with state confirmed, confirmed_by_person and confirm_reason. Since v0.8.6.

    Args:
        act (UUID):
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        body (ConfirmAdminActBody):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        AdminAct | ErrorEnvelope
    """

    return (
        await asyncio_detailed(
            act=act,
            client=client,
            body=body,
            x_request_id=x_request_id,
            x_correlation_id=x_correlation_id,
            traceparent=traceparent,
        )
    ).parsed
