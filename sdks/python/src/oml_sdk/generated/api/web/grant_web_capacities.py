from http import HTTPStatus
from typing import Any
from urllib.parse import quote

import httpx

from ...client import AuthenticatedClient, Client
from ...models.error_envelope import ErrorEnvelope
from ...models.grant_web_capacities_body import GrantWebCapacitiesBody
from ...models.grant_web_capacities_response_200 import GrantWebCapacitiesResponse200
from ...types import UNSET, Response, Unset


def _get_kwargs(
    user: str,
    *,
    body: GrantWebCapacitiesBody,
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
        "url": "/v2/web/users/{user}/capacities".format(
            user=quote(str(user), safe=""),
        ),
    }

    _kwargs["json"] = body.to_dict()

    headers["Content-Type"] = "application/json"

    _kwargs["headers"] = headers
    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> ErrorEnvelope | GrantWebCapacitiesResponse200:
    if response.status_code == 200:
        response_200 = GrantWebCapacitiesResponse200.from_dict(response.json())

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
) -> Response[ErrorEnvelope | GrantWebCapacitiesResponse200]:
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
    body: GrantWebCapacitiesBody,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
    idempotency_key: str,
) -> Response[ErrorEnvelope | GrantWebCapacitiesResponse200]:
    """Give a person's session key a capacity on workareas, review or curation.

     Requires: admin key role (ADR-0045 d10; ADR-0044 d7; ADR-0070 d5). review: <user>-review gains
    dreamer and reader on each workarea; curate: <user>-curate, a tenant writer key with writer on the
    universe node alone, is created at the first designation, and gains reader on each workarea. Each
    grant is one grant.issued; a grant the key holds already is not added again, so a repeat adds none
    and journal_seq is null. A user with no session key of that capacity answers access.not_found. This
    is the one path that creates a session key; POST /v2/keys never mints one. A resubmission under the
    same Idempotency-Key answers the recorded response with Idempotency-Replay: true and acts nothing
    again, the outcome recorded after the act's last commit; the same key on another request is
    idempotency.digest_mismatch (ADR-0070 d3 layer 4). Since v0.6.4.

    Args:
        user (str):
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        idempotency_key (str):
        body (GrantWebCapacitiesBody):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | GrantWebCapacitiesResponse200]
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
    body: GrantWebCapacitiesBody,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
    idempotency_key: str,
) -> ErrorEnvelope | GrantWebCapacitiesResponse200 | None:
    """Give a person's session key a capacity on workareas, review or curation.

     Requires: admin key role (ADR-0045 d10; ADR-0044 d7; ADR-0070 d5). review: <user>-review gains
    dreamer and reader on each workarea; curate: <user>-curate, a tenant writer key with writer on the
    universe node alone, is created at the first designation, and gains reader on each workarea. Each
    grant is one grant.issued; a grant the key holds already is not added again, so a repeat adds none
    and journal_seq is null. A user with no session key of that capacity answers access.not_found. This
    is the one path that creates a session key; POST /v2/keys never mints one. A resubmission under the
    same Idempotency-Key answers the recorded response with Idempotency-Replay: true and acts nothing
    again, the outcome recorded after the act's last commit; the same key on another request is
    idempotency.digest_mismatch (ADR-0070 d3 layer 4). Since v0.6.4.

    Args:
        user (str):
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        idempotency_key (str):
        body (GrantWebCapacitiesBody):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | GrantWebCapacitiesResponse200
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
    body: GrantWebCapacitiesBody,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
    idempotency_key: str,
) -> Response[ErrorEnvelope | GrantWebCapacitiesResponse200]:
    """Give a person's session key a capacity on workareas, review or curation.

     Requires: admin key role (ADR-0045 d10; ADR-0044 d7; ADR-0070 d5). review: <user>-review gains
    dreamer and reader on each workarea; curate: <user>-curate, a tenant writer key with writer on the
    universe node alone, is created at the first designation, and gains reader on each workarea. Each
    grant is one grant.issued; a grant the key holds already is not added again, so a repeat adds none
    and journal_seq is null. A user with no session key of that capacity answers access.not_found. This
    is the one path that creates a session key; POST /v2/keys never mints one. A resubmission under the
    same Idempotency-Key answers the recorded response with Idempotency-Replay: true and acts nothing
    again, the outcome recorded after the act's last commit; the same key on another request is
    idempotency.digest_mismatch (ADR-0070 d3 layer 4). Since v0.6.4.

    Args:
        user (str):
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        idempotency_key (str):
        body (GrantWebCapacitiesBody):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | GrantWebCapacitiesResponse200]
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
    body: GrantWebCapacitiesBody,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
    idempotency_key: str,
) -> ErrorEnvelope | GrantWebCapacitiesResponse200 | None:
    """Give a person's session key a capacity on workareas, review or curation.

     Requires: admin key role (ADR-0045 d10; ADR-0044 d7; ADR-0070 d5). review: <user>-review gains
    dreamer and reader on each workarea; curate: <user>-curate, a tenant writer key with writer on the
    universe node alone, is created at the first designation, and gains reader on each workarea. Each
    grant is one grant.issued; a grant the key holds already is not added again, so a repeat adds none
    and journal_seq is null. A user with no session key of that capacity answers access.not_found. This
    is the one path that creates a session key; POST /v2/keys never mints one. A resubmission under the
    same Idempotency-Key answers the recorded response with Idempotency-Replay: true and acts nothing
    again, the outcome recorded after the act's last commit; the same key on another request is
    idempotency.digest_mismatch (ADR-0070 d3 layer 4). Since v0.6.4.

    Args:
        user (str):
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        idempotency_key (str):
        body (GrantWebCapacitiesBody):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | GrantWebCapacitiesResponse200
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
