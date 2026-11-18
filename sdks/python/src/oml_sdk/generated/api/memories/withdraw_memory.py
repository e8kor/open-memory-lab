from http import HTTPStatus
from typing import Any
from urllib.parse import quote

import httpx

from ...client import AuthenticatedClient, Client
from ...models.error_envelope import ErrorEnvelope
from ...models.withdraw_request import WithdrawRequest
from ...models.withdraw_result import WithdrawResult
from ...types import UNSET, Response, Unset


def _get_kwargs(
    artifact_id: str,
    *,
    body: WithdrawRequest | Unset = UNSET,
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
        "url": "/v1/memories/{artifact_id}/withdraw".format(
            artifact_id=quote(str(artifact_id), safe=""),
        ),
    }

    if not isinstance(body, Unset):
        _kwargs["json"] = body.to_dict()

    headers["Content-Type"] = "application/json"

    _kwargs["headers"] = headers
    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> ErrorEnvelope | WithdrawResult:
    if response.status_code == 200:
        response_200 = WithdrawResult.from_dict(response.json())

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

    if response.status_code == 415:
        response_415 = ErrorEnvelope.from_dict(response.json())

        return response_415

    response_default = ErrorEnvelope.from_dict(response.json())

    return response_default


def _build_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Response[ErrorEnvelope | WithdrawResult]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    artifact_id: str,
    *,
    client: AuthenticatedClient | Client,
    body: WithdrawRequest | Unset = UNSET,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
    idempotency_key: str,
) -> Response[ErrorEnvelope | WithdrawResult]:
    """Withdraw a held version its author wrote; it is kept as withdrawn and never served (ADR-0083 d13).

     Requires: the version's author key, an agent's included, or a person's session web key for a held
    version any key of that same person wrote, read as same_user reads it; any workspace, a frozen one
    included. Another key that can read the version is 403 access.author_only, whatever its state, and a
    key that cannot read it is 404 access.not_found. A version not held is 409 memory.not_held. The
    version's state becomes withdrawn: it is kept with its text, leaves every review queue and is never
    served (not in recall, not to others in a listing, not inside an in-progress workspace); it is
    listed to its author with state withdrawn. Journals memory.withdrawn {artifact_id, workspace_id,
    reason} under the withdrawing key; no verdict is recorded and nothing is promoted, signed, demoted
    or deleted (R1). The reason is scanned as a verdict's is. The body may be empty. Idempotency-Key
    required, scoped to the key with a nil workspace.

    Args:
        artifact_id (str):
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        idempotency_key (str):
        body (WithdrawRequest | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | WithdrawResult]
    """

    kwargs = _get_kwargs(
        artifact_id=artifact_id,
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
    artifact_id: str,
    *,
    client: AuthenticatedClient | Client,
    body: WithdrawRequest | Unset = UNSET,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
    idempotency_key: str,
) -> ErrorEnvelope | WithdrawResult | None:
    """Withdraw a held version its author wrote; it is kept as withdrawn and never served (ADR-0083 d13).

     Requires: the version's author key, an agent's included, or a person's session web key for a held
    version any key of that same person wrote, read as same_user reads it; any workspace, a frozen one
    included. Another key that can read the version is 403 access.author_only, whatever its state, and a
    key that cannot read it is 404 access.not_found. A version not held is 409 memory.not_held. The
    version's state becomes withdrawn: it is kept with its text, leaves every review queue and is never
    served (not in recall, not to others in a listing, not inside an in-progress workspace); it is
    listed to its author with state withdrawn. Journals memory.withdrawn {artifact_id, workspace_id,
    reason} under the withdrawing key; no verdict is recorded and nothing is promoted, signed, demoted
    or deleted (R1). The reason is scanned as a verdict's is. The body may be empty. Idempotency-Key
    required, scoped to the key with a nil workspace.

    Args:
        artifact_id (str):
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        idempotency_key (str):
        body (WithdrawRequest | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | WithdrawResult
    """

    return sync_detailed(
        artifact_id=artifact_id,
        client=client,
        body=body,
        x_request_id=x_request_id,
        x_correlation_id=x_correlation_id,
        traceparent=traceparent,
        idempotency_key=idempotency_key,
    ).parsed


async def asyncio_detailed(
    artifact_id: str,
    *,
    client: AuthenticatedClient | Client,
    body: WithdrawRequest | Unset = UNSET,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
    idempotency_key: str,
) -> Response[ErrorEnvelope | WithdrawResult]:
    """Withdraw a held version its author wrote; it is kept as withdrawn and never served (ADR-0083 d13).

     Requires: the version's author key, an agent's included, or a person's session web key for a held
    version any key of that same person wrote, read as same_user reads it; any workspace, a frozen one
    included. Another key that can read the version is 403 access.author_only, whatever its state, and a
    key that cannot read it is 404 access.not_found. A version not held is 409 memory.not_held. The
    version's state becomes withdrawn: it is kept with its text, leaves every review queue and is never
    served (not in recall, not to others in a listing, not inside an in-progress workspace); it is
    listed to its author with state withdrawn. Journals memory.withdrawn {artifact_id, workspace_id,
    reason} under the withdrawing key; no verdict is recorded and nothing is promoted, signed, demoted
    or deleted (R1). The reason is scanned as a verdict's is. The body may be empty. Idempotency-Key
    required, scoped to the key with a nil workspace.

    Args:
        artifact_id (str):
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        idempotency_key (str):
        body (WithdrawRequest | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | WithdrawResult]
    """

    kwargs = _get_kwargs(
        artifact_id=artifact_id,
        body=body,
        x_request_id=x_request_id,
        x_correlation_id=x_correlation_id,
        traceparent=traceparent,
        idempotency_key=idempotency_key,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    artifact_id: str,
    *,
    client: AuthenticatedClient | Client,
    body: WithdrawRequest | Unset = UNSET,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
    idempotency_key: str,
) -> ErrorEnvelope | WithdrawResult | None:
    """Withdraw a held version its author wrote; it is kept as withdrawn and never served (ADR-0083 d13).

     Requires: the version's author key, an agent's included, or a person's session web key for a held
    version any key of that same person wrote, read as same_user reads it; any workspace, a frozen one
    included. Another key that can read the version is 403 access.author_only, whatever its state, and a
    key that cannot read it is 404 access.not_found. A version not held is 409 memory.not_held. The
    version's state becomes withdrawn: it is kept with its text, leaves every review queue and is never
    served (not in recall, not to others in a listing, not inside an in-progress workspace); it is
    listed to its author with state withdrawn. Journals memory.withdrawn {artifact_id, workspace_id,
    reason} under the withdrawing key; no verdict is recorded and nothing is promoted, signed, demoted
    or deleted (R1). The reason is scanned as a verdict's is. The body may be empty. Idempotency-Key
    required, scoped to the key with a nil workspace.

    Args:
        artifact_id (str):
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        idempotency_key (str):
        body (WithdrawRequest | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | WithdrawResult
    """

    return (
        await asyncio_detailed(
            artifact_id=artifact_id,
            client=client,
            body=body,
            x_request_id=x_request_id,
            x_correlation_id=x_correlation_id,
            traceparent=traceparent,
            idempotency_key=idempotency_key,
        )
    ).parsed
