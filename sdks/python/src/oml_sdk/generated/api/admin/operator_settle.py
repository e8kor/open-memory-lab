from http import HTTPStatus
from typing import Any
from urllib.parse import quote
from uuid import UUID

import httpx

from ...client import AuthenticatedClient, Client
from ...models.error_envelope import ErrorEnvelope
from ...models.operator_settle_body import OperatorSettleBody
from ...models.operator_settle_response_200 import OperatorSettleResponse200
from ...types import UNSET, Response, Unset


def _get_kwargs(
    tenant: UUID,
    act: UUID,
    *,
    body: OperatorSettleBody,
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
        "url": "/v2/operator/tenants/{tenant}/acts/{act}/settle".format(
            tenant=quote(str(tenant), safe=""),
            act=quote(str(act), safe=""),
        ),
    }

    _kwargs["json"] = body.to_dict()

    headers["Content-Type"] = "application/json"

    _kwargs["headers"] = headers
    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> ErrorEnvelope | OperatorSettleResponse200:
    if response.status_code == 200:
        response_200 = OperatorSettleResponse200.from_dict(response.json())

        return response_200

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
) -> Response[ErrorEnvelope | OperatorSettleResponse200]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    tenant: UUID,
    act: UUID,
    *,
    client: AuthenticatedClient | Client,
    body: OperatorSettleBody,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
    oml_operator_token: str,
) -> Response[ErrorEnvelope | OperatorSettleResponse200]:
    """The operator, from the host, confirms or reverses the contested act of a freeze open at least 24
    hours.

     Authenticated by the tenant's operator token of this start alone, sent as OML-Operator-Token: at
    each start the server writes one per tenant to <operator.token_dir>/<tenant>.token, mode 0600, and
    oml operator settle, run as root, reads it (ADR-0097 d19; the operator's answer 13 of 2026-11-27). A
    request that also sends Authorization is auth.invalid with details.reason
    operator_route_takes_no_key; no token, auth.missing; a token that is not the tenant's of this start,
    auth.invalid with details.reason operator_token. The act must be an open act against an admin below
    three admin persons, a freeze, else request.validation with details.reason not_frozen; and open at
    least 24 hours, a placeholder, else details.reason freeze_too_recent. confirm revokes the target's
    suspended admin keys, cause settlement, and undoes a contest's restoration; reverse lifts the
    suspension and restores the entry where no contest did. It issues no key and reads no content.
    Journalled in one commit: admin.act with state settled and no settler, operator.settlement, the
    effect's events, and notice.written kind settlement to every admin person and both parties. The
    notice is then emailed to them through the mail relay (d20; answer 14): mail is sent, unconfigured
    or failed, with mail_failure naming the SMTP stage and code, and the settlement stands either way.
    Since v0.8.6.

    Args:
        tenant (UUID):
        act (UUID):
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        oml_operator_token (str):
        body (OperatorSettleBody):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | OperatorSettleResponse200]
    """

    kwargs = _get_kwargs(
        tenant=tenant,
        act=act,
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
    act: UUID,
    *,
    client: AuthenticatedClient | Client,
    body: OperatorSettleBody,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
    oml_operator_token: str,
) -> ErrorEnvelope | OperatorSettleResponse200 | None:
    """The operator, from the host, confirms or reverses the contested act of a freeze open at least 24
    hours.

     Authenticated by the tenant's operator token of this start alone, sent as OML-Operator-Token: at
    each start the server writes one per tenant to <operator.token_dir>/<tenant>.token, mode 0600, and
    oml operator settle, run as root, reads it (ADR-0097 d19; the operator's answer 13 of 2026-11-27). A
    request that also sends Authorization is auth.invalid with details.reason
    operator_route_takes_no_key; no token, auth.missing; a token that is not the tenant's of this start,
    auth.invalid with details.reason operator_token. The act must be an open act against an admin below
    three admin persons, a freeze, else request.validation with details.reason not_frozen; and open at
    least 24 hours, a placeholder, else details.reason freeze_too_recent. confirm revokes the target's
    suspended admin keys, cause settlement, and undoes a contest's restoration; reverse lifts the
    suspension and restores the entry where no contest did. It issues no key and reads no content.
    Journalled in one commit: admin.act with state settled and no settler, operator.settlement, the
    effect's events, and notice.written kind settlement to every admin person and both parties. The
    notice is then emailed to them through the mail relay (d20; answer 14): mail is sent, unconfigured
    or failed, with mail_failure naming the SMTP stage and code, and the settlement stands either way.
    Since v0.8.6.

    Args:
        tenant (UUID):
        act (UUID):
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        oml_operator_token (str):
        body (OperatorSettleBody):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | OperatorSettleResponse200
    """

    return sync_detailed(
        tenant=tenant,
        act=act,
        client=client,
        body=body,
        x_request_id=x_request_id,
        x_correlation_id=x_correlation_id,
        traceparent=traceparent,
        oml_operator_token=oml_operator_token,
    ).parsed


async def asyncio_detailed(
    tenant: UUID,
    act: UUID,
    *,
    client: AuthenticatedClient | Client,
    body: OperatorSettleBody,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
    oml_operator_token: str,
) -> Response[ErrorEnvelope | OperatorSettleResponse200]:
    """The operator, from the host, confirms or reverses the contested act of a freeze open at least 24
    hours.

     Authenticated by the tenant's operator token of this start alone, sent as OML-Operator-Token: at
    each start the server writes one per tenant to <operator.token_dir>/<tenant>.token, mode 0600, and
    oml operator settle, run as root, reads it (ADR-0097 d19; the operator's answer 13 of 2026-11-27). A
    request that also sends Authorization is auth.invalid with details.reason
    operator_route_takes_no_key; no token, auth.missing; a token that is not the tenant's of this start,
    auth.invalid with details.reason operator_token. The act must be an open act against an admin below
    three admin persons, a freeze, else request.validation with details.reason not_frozen; and open at
    least 24 hours, a placeholder, else details.reason freeze_too_recent. confirm revokes the target's
    suspended admin keys, cause settlement, and undoes a contest's restoration; reverse lifts the
    suspension and restores the entry where no contest did. It issues no key and reads no content.
    Journalled in one commit: admin.act with state settled and no settler, operator.settlement, the
    effect's events, and notice.written kind settlement to every admin person and both parties. The
    notice is then emailed to them through the mail relay (d20; answer 14): mail is sent, unconfigured
    or failed, with mail_failure naming the SMTP stage and code, and the settlement stands either way.
    Since v0.8.6.

    Args:
        tenant (UUID):
        act (UUID):
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        oml_operator_token (str):
        body (OperatorSettleBody):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | OperatorSettleResponse200]
    """

    kwargs = _get_kwargs(
        tenant=tenant,
        act=act,
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
    act: UUID,
    *,
    client: AuthenticatedClient | Client,
    body: OperatorSettleBody,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
    oml_operator_token: str,
) -> ErrorEnvelope | OperatorSettleResponse200 | None:
    """The operator, from the host, confirms or reverses the contested act of a freeze open at least 24
    hours.

     Authenticated by the tenant's operator token of this start alone, sent as OML-Operator-Token: at
    each start the server writes one per tenant to <operator.token_dir>/<tenant>.token, mode 0600, and
    oml operator settle, run as root, reads it (ADR-0097 d19; the operator's answer 13 of 2026-11-27). A
    request that also sends Authorization is auth.invalid with details.reason
    operator_route_takes_no_key; no token, auth.missing; a token that is not the tenant's of this start,
    auth.invalid with details.reason operator_token. The act must be an open act against an admin below
    three admin persons, a freeze, else request.validation with details.reason not_frozen; and open at
    least 24 hours, a placeholder, else details.reason freeze_too_recent. confirm revokes the target's
    suspended admin keys, cause settlement, and undoes a contest's restoration; reverse lifts the
    suspension and restores the entry where no contest did. It issues no key and reads no content.
    Journalled in one commit: admin.act with state settled and no settler, operator.settlement, the
    effect's events, and notice.written kind settlement to every admin person and both parties. The
    notice is then emailed to them through the mail relay (d20; answer 14): mail is sent, unconfigured
    or failed, with mail_failure naming the SMTP stage and code, and the settlement stands either way.
    Since v0.8.6.

    Args:
        tenant (UUID):
        act (UUID):
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        oml_operator_token (str):
        body (OperatorSettleBody):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | OperatorSettleResponse200
    """

    return (
        await asyncio_detailed(
            tenant=tenant,
            act=act,
            client=client,
            body=body,
            x_request_id=x_request_id,
            x_correlation_id=x_correlation_id,
            traceparent=traceparent,
            oml_operator_token=oml_operator_token,
        )
    ).parsed
