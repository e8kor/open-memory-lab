from http import HTTPStatus
from typing import Any
from urllib.parse import quote

import httpx

from ...client import AuthenticatedClient, Client
from ...models.close_web_deletion_body import CloseWebDeletionBody
from ...models.close_web_deletion_response_200 import CloseWebDeletionResponse200
from ...models.error_envelope import ErrorEnvelope
from ...types import UNSET, Response, Unset


def _get_kwargs(
    user: str,
    *,
    body: CloseWebDeletionBody,
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
        "url": "/v2/web/users/{user}/deletion/close".format(
            user=quote(str(user), safe=""),
        ),
    }

    _kwargs["json"] = body.to_dict()

    headers["Content-Type"] = "application/json"

    _kwargs["headers"] = headers
    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> CloseWebDeletionResponse200 | ErrorEnvelope:
    if response.status_code == 200:
        response_200 = CloseWebDeletionResponse200.from_dict(response.json())

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
) -> Response[CloseWebDeletionResponse200 | ErrorEnvelope]:
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
    body: CloseWebDeletionBody,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
    idempotency_key: str,
) -> Response[CloseWebDeletionResponse200 | ErrorEnvelope]:
    """Close a person's pending deletion request, disabling them first.

     Requires: admin key role (ADR-0068 d6; ADR-0070 d5). The person is disabled as POST
    /v2/web/users/{user}/disable does, then web.deletion_closed is journalled. A user with no pending
    request answers access.not_found. outcome is deleted, the one close this build makes. A resubmission
    under the same Idempotency-Key answers the recorded response with Idempotency-Replay: true and acts
    nothing again, the outcome recorded after the act's last commit; the same key on another request is
    idempotency.digest_mismatch (ADR-0070 d3 layer 4). Since v0.6.4.

    Args:
        user (str):
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        idempotency_key (str):
        body (CloseWebDeletionBody):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[CloseWebDeletionResponse200 | ErrorEnvelope]
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
    body: CloseWebDeletionBody,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
    idempotency_key: str,
) -> CloseWebDeletionResponse200 | ErrorEnvelope | None:
    """Close a person's pending deletion request, disabling them first.

     Requires: admin key role (ADR-0068 d6; ADR-0070 d5). The person is disabled as POST
    /v2/web/users/{user}/disable does, then web.deletion_closed is journalled. A user with no pending
    request answers access.not_found. outcome is deleted, the one close this build makes. A resubmission
    under the same Idempotency-Key answers the recorded response with Idempotency-Replay: true and acts
    nothing again, the outcome recorded after the act's last commit; the same key on another request is
    idempotency.digest_mismatch (ADR-0070 d3 layer 4). Since v0.6.4.

    Args:
        user (str):
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        idempotency_key (str):
        body (CloseWebDeletionBody):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        CloseWebDeletionResponse200 | ErrorEnvelope
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
    body: CloseWebDeletionBody,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
    idempotency_key: str,
) -> Response[CloseWebDeletionResponse200 | ErrorEnvelope]:
    """Close a person's pending deletion request, disabling them first.

     Requires: admin key role (ADR-0068 d6; ADR-0070 d5). The person is disabled as POST
    /v2/web/users/{user}/disable does, then web.deletion_closed is journalled. A user with no pending
    request answers access.not_found. outcome is deleted, the one close this build makes. A resubmission
    under the same Idempotency-Key answers the recorded response with Idempotency-Replay: true and acts
    nothing again, the outcome recorded after the act's last commit; the same key on another request is
    idempotency.digest_mismatch (ADR-0070 d3 layer 4). Since v0.6.4.

    Args:
        user (str):
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        idempotency_key (str):
        body (CloseWebDeletionBody):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[CloseWebDeletionResponse200 | ErrorEnvelope]
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
    body: CloseWebDeletionBody,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
    idempotency_key: str,
) -> CloseWebDeletionResponse200 | ErrorEnvelope | None:
    """Close a person's pending deletion request, disabling them first.

     Requires: admin key role (ADR-0068 d6; ADR-0070 d5). The person is disabled as POST
    /v2/web/users/{user}/disable does, then web.deletion_closed is journalled. A user with no pending
    request answers access.not_found. outcome is deleted, the one close this build makes. A resubmission
    under the same Idempotency-Key answers the recorded response with Idempotency-Replay: true and acts
    nothing again, the outcome recorded after the act's last commit; the same key on another request is
    idempotency.digest_mismatch (ADR-0070 d3 layer 4). Since v0.6.4.

    Args:
        user (str):
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        idempotency_key (str):
        body (CloseWebDeletionBody):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        CloseWebDeletionResponse200 | ErrorEnvelope
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
