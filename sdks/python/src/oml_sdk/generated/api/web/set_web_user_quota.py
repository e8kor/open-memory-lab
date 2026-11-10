from http import HTTPStatus
from typing import Any
from urllib.parse import quote

import httpx

from ...client import AuthenticatedClient, Client
from ...models.error_envelope import ErrorEnvelope
from ...models.set_web_user_quota_body import SetWebUserQuotaBody
from ...models.web_user import WebUser
from ...types import UNSET, Response, Unset


def _get_kwargs(
    user: str,
    *,
    body: SetWebUserQuotaBody,
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
        "url": "/v2/web/users/{user}/quota".format(
            user=quote(str(user), safe=""),
        ),
    }

    _kwargs["json"] = body.to_dict()

    headers["Content-Type"] = "application/json"

    _kwargs["headers"] = headers
    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> ErrorEnvelope | WebUser:
    if response.status_code == 200:
        response_200 = WebUser.from_dict(response.json())

        return response_200

    if response.status_code == 403:
        response_403 = ErrorEnvelope.from_dict(response.json())

        return response_403

    if response.status_code == 404:
        response_404 = ErrorEnvelope.from_dict(response.json())

        return response_404

    response_default = ErrorEnvelope.from_dict(response.json())

    return response_default


def _build_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Response[ErrorEnvelope | WebUser]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    user: str,
    *,
    client: AuthenticatedClient | Client,
    body: SetWebUserQuotaBody,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
    idempotency_key: str,
) -> Response[ErrorEnvelope | WebUser]:
    """Set a person's quotas, or return one to the default with null.

     Requires: admin key role (ADR-0068 d5; ADR-0070 d5). Each quota sent is one web.quota_set; a member
    sent as null returns that quota to the default, and one not sent is left as it is. A limit is a
    whole number from 0 to 1000000, else request.validation naming it. A user with no user root answers
    access.not_found. Answers the person's row as GET /v2/web/users serves it. Since v0.6.4.

    Args:
        user (str):
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        idempotency_key (str):
        body (SetWebUserQuotaBody):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | WebUser]
    """

    kwargs = _get_kwargs(
        user=user,
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
    user: str,
    *,
    client: AuthenticatedClient | Client,
    body: SetWebUserQuotaBody,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
    idempotency_key: str,
) -> ErrorEnvelope | WebUser | None:
    """Set a person's quotas, or return one to the default with null.

     Requires: admin key role (ADR-0068 d5; ADR-0070 d5). Each quota sent is one web.quota_set; a member
    sent as null returns that quota to the default, and one not sent is left as it is. A limit is a
    whole number from 0 to 1000000, else request.validation naming it. A user with no user root answers
    access.not_found. Answers the person's row as GET /v2/web/users serves it. Since v0.6.4.

    Args:
        user (str):
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        idempotency_key (str):
        body (SetWebUserQuotaBody):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | WebUser
    """

    return sync_detailed(
        user=user,
        client=client,
        body=body,
        x_request_id=x_request_id,
        x_correlation_id=x_correlation_id,
        traceparent=traceparent,
        idempotency_key=idempotency_key,
    ).parsed


async def asyncio_detailed(
    user: str,
    *,
    client: AuthenticatedClient | Client,
    body: SetWebUserQuotaBody,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
    idempotency_key: str,
) -> Response[ErrorEnvelope | WebUser]:
    """Set a person's quotas, or return one to the default with null.

     Requires: admin key role (ADR-0068 d5; ADR-0070 d5). Each quota sent is one web.quota_set; a member
    sent as null returns that quota to the default, and one not sent is left as it is. A limit is a
    whole number from 0 to 1000000, else request.validation naming it. A user with no user root answers
    access.not_found. Answers the person's row as GET /v2/web/users serves it. Since v0.6.4.

    Args:
        user (str):
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        idempotency_key (str):
        body (SetWebUserQuotaBody):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | WebUser]
    """

    kwargs = _get_kwargs(
        user=user,
        body=body,
        x_request_id=x_request_id,
        x_correlation_id=x_correlation_id,
        traceparent=traceparent,
        idempotency_key=idempotency_key,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    user: str,
    *,
    client: AuthenticatedClient | Client,
    body: SetWebUserQuotaBody,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
    idempotency_key: str,
) -> ErrorEnvelope | WebUser | None:
    """Set a person's quotas, or return one to the default with null.

     Requires: admin key role (ADR-0068 d5; ADR-0070 d5). Each quota sent is one web.quota_set; a member
    sent as null returns that quota to the default, and one not sent is left as it is. A limit is a
    whole number from 0 to 1000000, else request.validation naming it. A user with no user root answers
    access.not_found. Answers the person's row as GET /v2/web/users serves it. Since v0.6.4.

    Args:
        user (str):
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        idempotency_key (str):
        body (SetWebUserQuotaBody):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | WebUser
    """

    return (
        await asyncio_detailed(
            user=user,
            client=client,
            body=body,
            x_request_id=x_request_id,
            x_correlation_id=x_correlation_id,
            traceparent=traceparent,
            idempotency_key=idempotency_key,
        )
    ).parsed
