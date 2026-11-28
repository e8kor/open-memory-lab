from http import HTTPStatus
from typing import Any
from urllib.parse import quote
from uuid import UUID

import httpx

from ...client import AuthenticatedClient, Client
from ...models.error_envelope import ErrorEnvelope
from ...models.operator_recover_body import OperatorRecoverBody
from ...models.operator_recover_response_202 import OperatorRecoverResponse202
from ...types import UNSET, Response, Unset


def _get_kwargs(
    tenant: UUID,
    *,
    body: OperatorRecoverBody,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
    oml_operator_token: str,
) -> dict[str, Any]:
    headers: dict[str, Any] = {}
    if not isinstance(x_request_id, Unset):
        headers["X-Request-Id"] = x_request_id

    if not isinstance(x_correlation_id, Unset):
        headers["X-Correlation-Id"] = x_correlation_id

    if not isinstance(traceparent, Unset):
        headers["traceparent"] = traceparent

    headers["OML-Operator-Token"] = oml_operator_token

    _kwargs: dict[str, Any] = {
        "method": "post",
        "url": "/v2/operator/tenants/{tenant}/recover".format(
            tenant=quote(str(tenant), safe=""),
        ),
    }

    _kwargs["json"] = body.to_dict()

    headers["Content-Type"] = "application/json"

    _kwargs["headers"] = headers
    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> ErrorEnvelope | OperatorRecoverResponse202:
    if response.status_code == 202:
        response_202 = OperatorRecoverResponse202.from_dict(response.json())

        return response_202

    if response.status_code == 401:
        response_401 = ErrorEnvelope.from_dict(response.json())

        return response_401

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
) -> Response[ErrorEnvelope | OperatorRecoverResponse202]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    tenant: UUID,
    *,
    client: AuthenticatedClient | Client,
    body: OperatorRecoverBody,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
    oml_operator_token: str,
) -> Response[ErrorEnvelope | OperatorRecoverResponse202]:
    """The operator, from the host, asks to recover a tenant in which no admin authenticated for 30 days.

     Authenticated as operatorSettle is, by the tenant's operator token of this start alone, sent as OML-
    Operator-Token, and refused auth.invalid with details.reason operator_route_takes_no_key beside an
    Authorization header (ADR-0097 d19; answer 13). Admitted only when the server authentication table
    reaches back 30 days, a placeholder, and shows no admin key of the tenant authenticated in them, an
    admin person's sign-in included (answers 21, 24). Refused operator.recover_refused with
    details.reason log_too_short (the table does not reach back 30 days: a fresh install or purged rows,
    never read as dormancy), log_write_failed (this server process counted an authentication row it
    could not write since it started; per replica), admin_active, pending (a recovery already waits),
    dispute_open (an act against an admin is open), person_taken or email_taken. Journalled as
    operator.recovery_requested, with the address's digest alone, and notice.written kind recovery to
    every admin person, then emailed to them through the mail relay (d20): mail is sent, unconfigured or
    failed. The server's sweep completes it once 72 hours, a placeholder, pass with no admin sign-in:
    every admin key of the tenant revoked, cause recovery, the named admin person's entry added,
    operator.recovery and a notice; any admin sign-in in the 72 hours cancels it,
    operator.recovery_cancelled (answer 22). The dormant admins keep their flags. Since v0.8.6.

    Args:
        tenant (UUID):
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        oml_operator_token (str):
        body (OperatorRecoverBody):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | OperatorRecoverResponse202]
    """

    kwargs = _get_kwargs(
        tenant=tenant,
        body=body,
        x_request_id=x_request_id,
        x_correlation_id=x_correlation_id,
        traceparent=traceparent,
        oml_operator_token=oml_operator_token,
    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    tenant: UUID,
    *,
    client: AuthenticatedClient | Client,
    body: OperatorRecoverBody,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
    oml_operator_token: str,
) -> ErrorEnvelope | OperatorRecoverResponse202 | None:
    """The operator, from the host, asks to recover a tenant in which no admin authenticated for 30 days.

     Authenticated as operatorSettle is, by the tenant's operator token of this start alone, sent as OML-
    Operator-Token, and refused auth.invalid with details.reason operator_route_takes_no_key beside an
    Authorization header (ADR-0097 d19; answer 13). Admitted only when the server authentication table
    reaches back 30 days, a placeholder, and shows no admin key of the tenant authenticated in them, an
    admin person's sign-in included (answers 21, 24). Refused operator.recover_refused with
    details.reason log_too_short (the table does not reach back 30 days: a fresh install or purged rows,
    never read as dormancy), log_write_failed (this server process counted an authentication row it
    could not write since it started; per replica), admin_active, pending (a recovery already waits),
    dispute_open (an act against an admin is open), person_taken or email_taken. Journalled as
    operator.recovery_requested, with the address's digest alone, and notice.written kind recovery to
    every admin person, then emailed to them through the mail relay (d20): mail is sent, unconfigured or
    failed. The server's sweep completes it once 72 hours, a placeholder, pass with no admin sign-in:
    every admin key of the tenant revoked, cause recovery, the named admin person's entry added,
    operator.recovery and a notice; any admin sign-in in the 72 hours cancels it,
    operator.recovery_cancelled (answer 22). The dormant admins keep their flags. Since v0.8.6.

    Args:
        tenant (UUID):
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        oml_operator_token (str):
        body (OperatorRecoverBody):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | OperatorRecoverResponse202
    """

    return sync_detailed(
        tenant=tenant,
        client=client,
        body=body,
        x_request_id=x_request_id,
        x_correlation_id=x_correlation_id,
        traceparent=traceparent,
        oml_operator_token=oml_operator_token,
    ).parsed


async def asyncio_detailed(
    tenant: UUID,
    *,
    client: AuthenticatedClient | Client,
    body: OperatorRecoverBody,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
    oml_operator_token: str,
) -> Response[ErrorEnvelope | OperatorRecoverResponse202]:
    """The operator, from the host, asks to recover a tenant in which no admin authenticated for 30 days.

     Authenticated as operatorSettle is, by the tenant's operator token of this start alone, sent as OML-
    Operator-Token, and refused auth.invalid with details.reason operator_route_takes_no_key beside an
    Authorization header (ADR-0097 d19; answer 13). Admitted only when the server authentication table
    reaches back 30 days, a placeholder, and shows no admin key of the tenant authenticated in them, an
    admin person's sign-in included (answers 21, 24). Refused operator.recover_refused with
    details.reason log_too_short (the table does not reach back 30 days: a fresh install or purged rows,
    never read as dormancy), log_write_failed (this server process counted an authentication row it
    could not write since it started; per replica), admin_active, pending (a recovery already waits),
    dispute_open (an act against an admin is open), person_taken or email_taken. Journalled as
    operator.recovery_requested, with the address's digest alone, and notice.written kind recovery to
    every admin person, then emailed to them through the mail relay (d20): mail is sent, unconfigured or
    failed. The server's sweep completes it once 72 hours, a placeholder, pass with no admin sign-in:
    every admin key of the tenant revoked, cause recovery, the named admin person's entry added,
    operator.recovery and a notice; any admin sign-in in the 72 hours cancels it,
    operator.recovery_cancelled (answer 22). The dormant admins keep their flags. Since v0.8.6.

    Args:
        tenant (UUID):
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        oml_operator_token (str):
        body (OperatorRecoverBody):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | OperatorRecoverResponse202]
    """

    kwargs = _get_kwargs(
        tenant=tenant,
        body=body,
        x_request_id=x_request_id,
        x_correlation_id=x_correlation_id,
        traceparent=traceparent,
        oml_operator_token=oml_operator_token,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    tenant: UUID,
    *,
    client: AuthenticatedClient | Client,
    body: OperatorRecoverBody,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
    oml_operator_token: str,
) -> ErrorEnvelope | OperatorRecoverResponse202 | None:
    """The operator, from the host, asks to recover a tenant in which no admin authenticated for 30 days.

     Authenticated as operatorSettle is, by the tenant's operator token of this start alone, sent as OML-
    Operator-Token, and refused auth.invalid with details.reason operator_route_takes_no_key beside an
    Authorization header (ADR-0097 d19; answer 13). Admitted only when the server authentication table
    reaches back 30 days, a placeholder, and shows no admin key of the tenant authenticated in them, an
    admin person's sign-in included (answers 21, 24). Refused operator.recover_refused with
    details.reason log_too_short (the table does not reach back 30 days: a fresh install or purged rows,
    never read as dormancy), log_write_failed (this server process counted an authentication row it
    could not write since it started; per replica), admin_active, pending (a recovery already waits),
    dispute_open (an act against an admin is open), person_taken or email_taken. Journalled as
    operator.recovery_requested, with the address's digest alone, and notice.written kind recovery to
    every admin person, then emailed to them through the mail relay (d20): mail is sent, unconfigured or
    failed. The server's sweep completes it once 72 hours, a placeholder, pass with no admin sign-in:
    every admin key of the tenant revoked, cause recovery, the named admin person's entry added,
    operator.recovery and a notice; any admin sign-in in the 72 hours cancels it,
    operator.recovery_cancelled (answer 22). The dormant admins keep their flags. Since v0.8.6.

    Args:
        tenant (UUID):
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        oml_operator_token (str):
        body (OperatorRecoverBody):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | OperatorRecoverResponse202
    """

    return (
        await asyncio_detailed(
            tenant=tenant,
            client=client,
            body=body,
            x_request_id=x_request_id,
            x_correlation_id=x_correlation_id,
            traceparent=traceparent,
            oml_operator_token=oml_operator_token,
        )
    ).parsed
