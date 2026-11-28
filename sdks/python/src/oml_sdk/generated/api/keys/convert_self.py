from http import HTTPStatus
from typing import Any

import httpx

from ...client import AuthenticatedClient, Client
from ...models.convert_self_body import ConvertSelfBody
from ...models.convert_self_response_200 import ConvertSelfResponse200
from ...models.error_envelope import ErrorEnvelope
from ...types import UNSET, Response, Unset


def _get_kwargs(
    *,
    body: ConvertSelfBody,
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
        "url": "/v2/keys/self/convert",
    }

    _kwargs["json"] = body.to_dict()

    headers["Content-Type"] = "application/json"

    _kwargs["headers"] = headers
    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> ConvertSelfResponse200 | ErrorEnvelope:
    if response.status_code == 200:
        response_200 = ConvertSelfResponse200.from_dict(response.json())

        return response_200

    if response.status_code == 403:
        response_403 = ErrorEnvelope.from_dict(response.json())

        return response_403

    response_default = ErrorEnvelope.from_dict(response.json())

    return response_default


def _build_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Response[ConvertSelfResponse200 | ErrorEnvelope]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    *,
    client: AuthenticatedClient | Client,
    body: ConvertSelfBody,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> Response[ConvertSelfResponse200 | ErrorEnvelope]:
    """Convert the calling admin key, in place, into a maintenance key that reads counts only.

     Requires: an admin key presented by its own current bearer value; any other role is
    request.validation with details.field key and details.reason not_admin, and a session key
    details.reason session_key (ADR-0097 d18). to is maintenance; anything else is details.field /to and
    details.reason not_maintenance. The key keeps its id, name and hash, takes the maintenance role's
    request rate, 60 a minute (a placeholder), and from then on reaches only the counts oml doctor
    reads: GET /v2/whoami, GET and POST /v2/admin/rescan, GET /v2/admin/usage/drift, GET /v2/search, GET
    /v2/federation with no key listed, GET /v2/workspaces/{workspace}/tree, GET /v2/review/queue, GET
    /v2/events with the head and no event, and GET /v2/web/users with no key listed and no deletion
    named; every other route refuses it access.verb_ceiling with details.verb maintenance_counts_only.
    Journalled as one sealed key.converted {key_id, from: admin, to: maintenance}, never a migration's
    UPDATE. Since v0.8.6.

    Args:
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        body (ConvertSelfBody):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ConvertSelfResponse200 | ErrorEnvelope]
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
    body: ConvertSelfBody,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> ConvertSelfResponse200 | ErrorEnvelope | None:
    """Convert the calling admin key, in place, into a maintenance key that reads counts only.

     Requires: an admin key presented by its own current bearer value; any other role is
    request.validation with details.field key and details.reason not_admin, and a session key
    details.reason session_key (ADR-0097 d18). to is maintenance; anything else is details.field /to and
    details.reason not_maintenance. The key keeps its id, name and hash, takes the maintenance role's
    request rate, 60 a minute (a placeholder), and from then on reaches only the counts oml doctor
    reads: GET /v2/whoami, GET and POST /v2/admin/rescan, GET /v2/admin/usage/drift, GET /v2/search, GET
    /v2/federation with no key listed, GET /v2/workspaces/{workspace}/tree, GET /v2/review/queue, GET
    /v2/events with the head and no event, and GET /v2/web/users with no key listed and no deletion
    named; every other route refuses it access.verb_ceiling with details.verb maintenance_counts_only.
    Journalled as one sealed key.converted {key_id, from: admin, to: maintenance}, never a migration's
    UPDATE. Since v0.8.6.

    Args:
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        body (ConvertSelfBody):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ConvertSelfResponse200 | ErrorEnvelope
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
    body: ConvertSelfBody,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> Response[ConvertSelfResponse200 | ErrorEnvelope]:
    """Convert the calling admin key, in place, into a maintenance key that reads counts only.

     Requires: an admin key presented by its own current bearer value; any other role is
    request.validation with details.field key and details.reason not_admin, and a session key
    details.reason session_key (ADR-0097 d18). to is maintenance; anything else is details.field /to and
    details.reason not_maintenance. The key keeps its id, name and hash, takes the maintenance role's
    request rate, 60 a minute (a placeholder), and from then on reaches only the counts oml doctor
    reads: GET /v2/whoami, GET and POST /v2/admin/rescan, GET /v2/admin/usage/drift, GET /v2/search, GET
    /v2/federation with no key listed, GET /v2/workspaces/{workspace}/tree, GET /v2/review/queue, GET
    /v2/events with the head and no event, and GET /v2/web/users with no key listed and no deletion
    named; every other route refuses it access.verb_ceiling with details.verb maintenance_counts_only.
    Journalled as one sealed key.converted {key_id, from: admin, to: maintenance}, never a migration's
    UPDATE. Since v0.8.6.

    Args:
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        body (ConvertSelfBody):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ConvertSelfResponse200 | ErrorEnvelope]
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
    body: ConvertSelfBody,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> ConvertSelfResponse200 | ErrorEnvelope | None:
    """Convert the calling admin key, in place, into a maintenance key that reads counts only.

     Requires: an admin key presented by its own current bearer value; any other role is
    request.validation with details.field key and details.reason not_admin, and a session key
    details.reason session_key (ADR-0097 d18). to is maintenance; anything else is details.field /to and
    details.reason not_maintenance. The key keeps its id, name and hash, takes the maintenance role's
    request rate, 60 a minute (a placeholder), and from then on reaches only the counts oml doctor
    reads: GET /v2/whoami, GET and POST /v2/admin/rescan, GET /v2/admin/usage/drift, GET /v2/search, GET
    /v2/federation with no key listed, GET /v2/workspaces/{workspace}/tree, GET /v2/review/queue, GET
    /v2/events with the head and no event, and GET /v2/web/users with no key listed and no deletion
    named; every other route refuses it access.verb_ceiling with details.verb maintenance_counts_only.
    Journalled as one sealed key.converted {key_id, from: admin, to: maintenance}, never a migration's
    UPDATE. Since v0.8.6.

    Args:
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        body (ConvertSelfBody):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ConvertSelfResponse200 | ErrorEnvelope
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
