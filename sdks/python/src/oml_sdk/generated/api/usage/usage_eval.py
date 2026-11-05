from http import HTTPStatus
from typing import Any

import httpx

from ...client import AuthenticatedClient, Client
from ...models.error_envelope import ErrorEnvelope
from ...models.replay_report import ReplayReport
from ...models.usage_eval_body import UsageEvalBody
from ...types import UNSET, Response, Unset


def _get_kwargs(
    *,
    body: UsageEvalBody,
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
        "url": "/v2/usage/eval",
    }

    _kwargs["json"] = body.to_dict()

    headers["Content-Type"] = "application/json"

    _kwargs["headers"] = headers
    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> ErrorEnvelope | ReplayReport:
    if response.status_code == 200:
        response_200 = ReplayReport.from_dict(response.json())

        return response_200

    if response.status_code == 403:
        response_403 = ErrorEnvelope.from_dict(response.json())

        return response_403

    response_default = ErrorEnvelope.from_dict(response.json())

    return response_default


def _build_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Response[ErrorEnvelope | ReplayReport]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    *,
    client: AuthenticatedClient | Client,
    body: UsageEvalBody,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> Response[ErrorEnvelope | ReplayReport]:
    """Replay the tenant's labelled recalls through each named pipeline and answer the scores alone.

     Requires: admin key role (ADR-0054 d11). Each recall of the last 30 days whose text was kept and
    which carries feedback, a get or a citation is replayed inside the server under its original key's
    scope as that key reads now; a revoked key's are skipped and counted. Graded: feedback useful and a
    citation 2, a get or a supersession 1, feedback not useful 0. The answer holds numbers and counts,
    never a query, a hit or an id, and the replay records no event. Read-shaped POST: no Idempotency-
    Key. Since v0.5.7.

    Args:
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        body (UsageEvalBody):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | ReplayReport]
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
    body: UsageEvalBody,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> ErrorEnvelope | ReplayReport | None:
    """Replay the tenant's labelled recalls through each named pipeline and answer the scores alone.

     Requires: admin key role (ADR-0054 d11). Each recall of the last 30 days whose text was kept and
    which carries feedback, a get or a citation is replayed inside the server under its original key's
    scope as that key reads now; a revoked key's are skipped and counted. Graded: feedback useful and a
    citation 2, a get or a supersession 1, feedback not useful 0. The answer holds numbers and counts,
    never a query, a hit or an id, and the replay records no event. Read-shaped POST: no Idempotency-
    Key. Since v0.5.7.

    Args:
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        body (UsageEvalBody):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | ReplayReport
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
    body: UsageEvalBody,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> Response[ErrorEnvelope | ReplayReport]:
    """Replay the tenant's labelled recalls through each named pipeline and answer the scores alone.

     Requires: admin key role (ADR-0054 d11). Each recall of the last 30 days whose text was kept and
    which carries feedback, a get or a citation is replayed inside the server under its original key's
    scope as that key reads now; a revoked key's are skipped and counted. Graded: feedback useful and a
    citation 2, a get or a supersession 1, feedback not useful 0. The answer holds numbers and counts,
    never a query, a hit or an id, and the replay records no event. Read-shaped POST: no Idempotency-
    Key. Since v0.5.7.

    Args:
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        body (UsageEvalBody):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | ReplayReport]
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
    body: UsageEvalBody,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> ErrorEnvelope | ReplayReport | None:
    """Replay the tenant's labelled recalls through each named pipeline and answer the scores alone.

     Requires: admin key role (ADR-0054 d11). Each recall of the last 30 days whose text was kept and
    which carries feedback, a get or a citation is replayed inside the server under its original key's
    scope as that key reads now; a revoked key's are skipped and counted. Graded: feedback useful and a
    citation 2, a get or a supersession 1, feedback not useful 0. The answer holds numbers and counts,
    never a query, a hit or an id, and the replay records no event. Read-shaped POST: no Idempotency-
    Key. Since v0.5.7.

    Args:
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        body (UsageEvalBody):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | ReplayReport
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
