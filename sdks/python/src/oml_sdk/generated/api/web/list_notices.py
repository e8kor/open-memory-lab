from http import HTTPStatus
from typing import Any

import httpx

from ...client import AuthenticatedClient, Client
from ...models.error_envelope import ErrorEnvelope
from ...models.list_notices_response_200 import ListNoticesResponse200
from ...types import UNSET, Response, Unset


def _get_kwargs(
    *,
    dismissed: bool | Unset = False,
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

    params: dict[str, Any] = {}

    params["dismissed"] = dismissed

    params = {k: v for k, v in params.items() if v is not UNSET and v is not None}

    _kwargs: dict[str, Any] = {
        "method": "get",
        "url": "/v2/web/notices",
        "params": params,
    }

    _kwargs["headers"] = headers
    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> ErrorEnvelope | ListNoticesResponse200:
    if response.status_code == 200:
        response_200 = ListNoticesResponse200.from_dict(response.json())

        return response_200

    if response.status_code == 401:
        response_401 = ErrorEnvelope.from_dict(response.json())

        return response_401

    if response.status_code == 404:
        response_404 = ErrorEnvelope.from_dict(response.json())

        return response_404

    response_default = ErrorEnvelope.from_dict(response.json())

    return response_default


def _build_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Response[ErrorEnvelope | ListNoticesResponse200]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    *,
    client: AuthenticatedClient | Client,
    dismissed: bool | Unset = False,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> Response[ErrorEnvelope | ListNoticesResponse200]:
    """The calling person's in-product notices.

     Requires: a key that counts as a person, the person's session keys among them; one that counts as no
    person answers access.not_found. A notice is written in the commit of the act it tells about: an act
    against an admin, to every admin person; offensive narrowing (a production fix that loosens a
    guardrail, disabling a person, revoking a person's key, denying a reviewer person's key), to every
    admin person and every reviewer person of the workspace concerned; a reach into a person's own root
    taken at once under sole_admin, to that person (ADR-0097 d6, d20, d24). Oldest first; the dismissed
    ones too with dismissed=true. Journals nothing. Since v0.8.6.

    Args:
        dismissed (bool | Unset):  Default: False.
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | ListNoticesResponse200]
    """

    kwargs = _get_kwargs(
        dismissed=dismissed,
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
    dismissed: bool | Unset = False,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> ErrorEnvelope | ListNoticesResponse200 | None:
    """The calling person's in-product notices.

     Requires: a key that counts as a person, the person's session keys among them; one that counts as no
    person answers access.not_found. A notice is written in the commit of the act it tells about: an act
    against an admin, to every admin person; offensive narrowing (a production fix that loosens a
    guardrail, disabling a person, revoking a person's key, denying a reviewer person's key), to every
    admin person and every reviewer person of the workspace concerned; a reach into a person's own root
    taken at once under sole_admin, to that person (ADR-0097 d6, d20, d24). Oldest first; the dismissed
    ones too with dismissed=true. Journals nothing. Since v0.8.6.

    Args:
        dismissed (bool | Unset):  Default: False.
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | ListNoticesResponse200
    """

    return sync_detailed(
        client=client,
        dismissed=dismissed,
        x_request_id=x_request_id,
        x_correlation_id=x_correlation_id,
        traceparent=traceparent,
    ).parsed


async def asyncio_detailed(
    *,
    client: AuthenticatedClient | Client,
    dismissed: bool | Unset = False,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> Response[ErrorEnvelope | ListNoticesResponse200]:
    """The calling person's in-product notices.

     Requires: a key that counts as a person, the person's session keys among them; one that counts as no
    person answers access.not_found. A notice is written in the commit of the act it tells about: an act
    against an admin, to every admin person; offensive narrowing (a production fix that loosens a
    guardrail, disabling a person, revoking a person's key, denying a reviewer person's key), to every
    admin person and every reviewer person of the workspace concerned; a reach into a person's own root
    taken at once under sole_admin, to that person (ADR-0097 d6, d20, d24). Oldest first; the dismissed
    ones too with dismissed=true. Journals nothing. Since v0.8.6.

    Args:
        dismissed (bool | Unset):  Default: False.
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | ListNoticesResponse200]
    """

    kwargs = _get_kwargs(
        dismissed=dismissed,
        x_request_id=x_request_id,
        x_correlation_id=x_correlation_id,
        traceparent=traceparent,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    *,
    client: AuthenticatedClient | Client,
    dismissed: bool | Unset = False,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> ErrorEnvelope | ListNoticesResponse200 | None:
    """The calling person's in-product notices.

     Requires: a key that counts as a person, the person's session keys among them; one that counts as no
    person answers access.not_found. A notice is written in the commit of the act it tells about: an act
    against an admin, to every admin person; offensive narrowing (a production fix that loosens a
    guardrail, disabling a person, revoking a person's key, denying a reviewer person's key), to every
    admin person and every reviewer person of the workspace concerned; a reach into a person's own root
    taken at once under sole_admin, to that person (ADR-0097 d6, d20, d24). Oldest first; the dismissed
    ones too with dismissed=true. Journals nothing. Since v0.8.6.

    Args:
        dismissed (bool | Unset):  Default: False.
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | ListNoticesResponse200
    """

    return (
        await asyncio_detailed(
            client=client,
            dismissed=dismissed,
            x_request_id=x_request_id,
            x_correlation_id=x_correlation_id,
            traceparent=traceparent,
        )
    ).parsed
