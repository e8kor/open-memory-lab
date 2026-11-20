from http import HTTPStatus
from typing import Any

import httpx

from ...client import AuthenticatedClient, Client
from ...models.accept_web_email_body import AcceptWebEmailBody
from ...models.error_envelope import ErrorEnvelope
from ...models.web_email import WebEmail
from ...types import UNSET, Response, Unset


def _get_kwargs(
    *,
    body: AcceptWebEmailBody,
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
        "url": "/v2/web/emails/accept",
    }

    _kwargs["json"] = body.to_dict()

    headers["Content-Type"] = "application/json"

    _kwargs["headers"] = headers
    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> ErrorEnvelope | WebEmail:
    if response.status_code == 200:
        response_200 = WebEmail.from_dict(response.json())

        return response_200

    if response.status_code == 404:
        response_404 = ErrorEnvelope.from_dict(response.json())

        return response_404

    response_default = ErrorEnvelope.from_dict(response.json())

    return response_default


def _build_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Response[ErrorEnvelope | WebEmail]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    *,
    client: AuthenticatedClient | Client,
    body: AcceptWebEmailBody,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
    idempotency_key: str,
) -> Response[ErrorEnvelope | WebEmail]:
    """Accept a sign-in email an admin added for the signed-in person, so it signs in as them.

     Requires: the console session of the person the pending entry names, through its capacity credential
    (the operator's decision of 2026-11-19, ADR-0045 d2's note). An email an admin adds for a person who
    signs in already, or an entry changed to name such a person, is pending and signs in to nothing
    until that person accepts it. Journalled as web.email_accepted with the email's digest; the email
    signs in as the person from then on. Refused request.validation, details.field /email, with
    details.reason session_only for a bearer key, an admin key's included, not_named_person for a
    session of any other person, the adding admin's included, not_pending for an entry that waits on no
    one, and changed (details.field /admin) for an entry whose admin flag is not the one sent; an email
    on no entry of the caller's tenant answers access.not_found. A refusal journals nothing. The email
    is in the body, never the path. Since v0.8.4.

    Args:
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        idempotency_key (str):
        body (AcceptWebEmailBody):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | WebEmail]
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
    body: AcceptWebEmailBody,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
    idempotency_key: str,
) -> ErrorEnvelope | WebEmail | None:
    """Accept a sign-in email an admin added for the signed-in person, so it signs in as them.

     Requires: the console session of the person the pending entry names, through its capacity credential
    (the operator's decision of 2026-11-19, ADR-0045 d2's note). An email an admin adds for a person who
    signs in already, or an entry changed to name such a person, is pending and signs in to nothing
    until that person accepts it. Journalled as web.email_accepted with the email's digest; the email
    signs in as the person from then on. Refused request.validation, details.field /email, with
    details.reason session_only for a bearer key, an admin key's included, not_named_person for a
    session of any other person, the adding admin's included, not_pending for an entry that waits on no
    one, and changed (details.field /admin) for an entry whose admin flag is not the one sent; an email
    on no entry of the caller's tenant answers access.not_found. A refusal journals nothing. The email
    is in the body, never the path. Since v0.8.4.

    Args:
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        idempotency_key (str):
        body (AcceptWebEmailBody):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | WebEmail
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
    body: AcceptWebEmailBody,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
    idempotency_key: str,
) -> Response[ErrorEnvelope | WebEmail]:
    """Accept a sign-in email an admin added for the signed-in person, so it signs in as them.

     Requires: the console session of the person the pending entry names, through its capacity credential
    (the operator's decision of 2026-11-19, ADR-0045 d2's note). An email an admin adds for a person who
    signs in already, or an entry changed to name such a person, is pending and signs in to nothing
    until that person accepts it. Journalled as web.email_accepted with the email's digest; the email
    signs in as the person from then on. Refused request.validation, details.field /email, with
    details.reason session_only for a bearer key, an admin key's included, not_named_person for a
    session of any other person, the adding admin's included, not_pending for an entry that waits on no
    one, and changed (details.field /admin) for an entry whose admin flag is not the one sent; an email
    on no entry of the caller's tenant answers access.not_found. A refusal journals nothing. The email
    is in the body, never the path. Since v0.8.4.

    Args:
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        idempotency_key (str):
        body (AcceptWebEmailBody):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | WebEmail]
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
    body: AcceptWebEmailBody,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
    idempotency_key: str,
) -> ErrorEnvelope | WebEmail | None:
    """Accept a sign-in email an admin added for the signed-in person, so it signs in as them.

     Requires: the console session of the person the pending entry names, through its capacity credential
    (the operator's decision of 2026-11-19, ADR-0045 d2's note). An email an admin adds for a person who
    signs in already, or an entry changed to name such a person, is pending and signs in to nothing
    until that person accepts it. Journalled as web.email_accepted with the email's digest; the email
    signs in as the person from then on. Refused request.validation, details.field /email, with
    details.reason session_only for a bearer key, an admin key's included, not_named_person for a
    session of any other person, the adding admin's included, not_pending for an entry that waits on no
    one, and changed (details.field /admin) for an entry whose admin flag is not the one sent; an email
    on no entry of the caller's tenant answers access.not_found. A refusal journals nothing. The email
    is in the body, never the path. Since v0.8.4.

    Args:
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        idempotency_key (str):
        body (AcceptWebEmailBody):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | WebEmail
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
