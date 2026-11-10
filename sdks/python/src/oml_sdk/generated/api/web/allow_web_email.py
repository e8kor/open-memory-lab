from http import HTTPStatus
from typing import Any

import httpx

from ...client import AuthenticatedClient, Client
from ...models.allow_web_email_body import AllowWebEmailBody
from ...models.error_envelope import ErrorEnvelope
from ...models.web_email import WebEmail
from ...types import UNSET, Response, Unset


def _get_kwargs(
    *,
    body: AllowWebEmailBody,
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
        "url": "/v2/web/emails",
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

    if response.status_code == 403:
        response_403 = ErrorEnvelope.from_dict(response.json())

        return response_403

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
    body: AllowWebEmailBody,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
    idempotency_key: str,
) -> Response[ErrorEnvelope | WebEmail]:
    """Add an email to the allowlist, or change the user or admin flag of one already there.

     Requires: admin key role (ADR-0045 d2). The email is unique in the deployment and compared ASCII
    case-insensitively (409 name.taken, details.kind email); user is a name under the creation grammar
    of at most 56 characters, refused 409 name.taken when a shared workarea holds it or when a key holds
    one of the names <user>-web, <user>-review, <user>-curate, <user>-admin. Journalled as
    web.email_allowed or web.email_changed with an email digest, never the address. A change ends the
    entry's sessions. Since v0.5.0.

    Args:
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        idempotency_key (str):
        body (AllowWebEmailBody):

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
    body: AllowWebEmailBody,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
    idempotency_key: str,
) -> ErrorEnvelope | WebEmail | None:
    """Add an email to the allowlist, or change the user or admin flag of one already there.

     Requires: admin key role (ADR-0045 d2). The email is unique in the deployment and compared ASCII
    case-insensitively (409 name.taken, details.kind email); user is a name under the creation grammar
    of at most 56 characters, refused 409 name.taken when a shared workarea holds it or when a key holds
    one of the names <user>-web, <user>-review, <user>-curate, <user>-admin. Journalled as
    web.email_allowed or web.email_changed with an email digest, never the address. A change ends the
    entry's sessions. Since v0.5.0.

    Args:
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        idempotency_key (str):
        body (AllowWebEmailBody):

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
    body: AllowWebEmailBody,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
    idempotency_key: str,
) -> Response[ErrorEnvelope | WebEmail]:
    """Add an email to the allowlist, or change the user or admin flag of one already there.

     Requires: admin key role (ADR-0045 d2). The email is unique in the deployment and compared ASCII
    case-insensitively (409 name.taken, details.kind email); user is a name under the creation grammar
    of at most 56 characters, refused 409 name.taken when a shared workarea holds it or when a key holds
    one of the names <user>-web, <user>-review, <user>-curate, <user>-admin. Journalled as
    web.email_allowed or web.email_changed with an email digest, never the address. A change ends the
    entry's sessions. Since v0.5.0.

    Args:
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        idempotency_key (str):
        body (AllowWebEmailBody):

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
    body: AllowWebEmailBody,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
    idempotency_key: str,
) -> ErrorEnvelope | WebEmail | None:
    """Add an email to the allowlist, or change the user or admin flag of one already there.

     Requires: admin key role (ADR-0045 d2). The email is unique in the deployment and compared ASCII
    case-insensitively (409 name.taken, details.kind email); user is a name under the creation grammar
    of at most 56 characters, refused 409 name.taken when a shared workarea holds it or when a key holds
    one of the names <user>-web, <user>-review, <user>-curate, <user>-admin. Journalled as
    web.email_allowed or web.email_changed with an email digest, never the address. A change ends the
    entry's sessions. Since v0.5.0.

    Args:
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        idempotency_key (str):
        body (AllowWebEmailBody):

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
