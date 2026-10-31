from http import HTTPStatus
from typing import Any
from urllib.parse import quote

import httpx

from ...client import AuthenticatedClient, Client
from ...models.error_envelope import ErrorEnvelope
from ...models.transition import Transition
from ...types import Response


def _get_kwargs(
    workarea_id: str,
    transition_id: str,
    *,
    idempotency_key: str,
) -> dict[str, Any]:
    headers: dict[str, Any] = {}
    headers["Idempotency-Key"] = idempotency_key

    _kwargs: dict[str, Any] = {
        "method": "post",
        "url": "/v2/workareas/{workarea_id}/transitions/{transition_id}/ratify".format(
            workarea_id=quote(str(workarea_id), safe=""),
            transition_id=quote(str(transition_id), safe=""),
        ),
    }

    _kwargs["headers"] = headers
    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> ErrorEnvelope | Transition:
    if response.status_code == 200:
        response_200 = Transition.from_dict(response.json())

        return response_200

    if response.status_code == 403:
        response_403 = ErrorEnvelope.from_dict(response.json())

        return response_403

    response_default = ErrorEnvelope.from_dict(response.json())

    return response_default


def _build_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Response[ErrorEnvelope | Transition]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    workarea_id: str,
    transition_id: str,
    *,
    client: AuthenticatedClient | Client,
    idempotency_key: str,
) -> Response[ErrorEnvelope | Transition]:
    """Ratify a pending transition; journalled with the policy snapshot.

     Requires: a second, non-proposing dreamer or admin (review.ratifier_is_proposer otherwise — R2
    extends to transitions). A held_by: service key is refused with access.verb_ceiling before any
    lookup (ADR-0025 d1).

    Args:
        workarea_id (str):
        transition_id (str):
        idempotency_key (str):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | Transition]
    """

    kwargs = _get_kwargs(
        workarea_id=workarea_id,
        transition_id=transition_id,
        idempotency_key=idempotency_key,
    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    workarea_id: str,
    transition_id: str,
    *,
    client: AuthenticatedClient | Client,
    idempotency_key: str,
) -> ErrorEnvelope | Transition | None:
    """Ratify a pending transition; journalled with the policy snapshot.

     Requires: a second, non-proposing dreamer or admin (review.ratifier_is_proposer otherwise — R2
    extends to transitions). A held_by: service key is refused with access.verb_ceiling before any
    lookup (ADR-0025 d1).

    Args:
        workarea_id (str):
        transition_id (str):
        idempotency_key (str):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | Transition
    """

    return sync_detailed(
        workarea_id=workarea_id,
        transition_id=transition_id,
        client=client,
        idempotency_key=idempotency_key,
    ).parsed


async def asyncio_detailed(
    workarea_id: str,
    transition_id: str,
    *,
    client: AuthenticatedClient | Client,
    idempotency_key: str,
) -> Response[ErrorEnvelope | Transition]:
    """Ratify a pending transition; journalled with the policy snapshot.

     Requires: a second, non-proposing dreamer or admin (review.ratifier_is_proposer otherwise — R2
    extends to transitions). A held_by: service key is refused with access.verb_ceiling before any
    lookup (ADR-0025 d1).

    Args:
        workarea_id (str):
        transition_id (str):
        idempotency_key (str):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | Transition]
    """

    kwargs = _get_kwargs(
        workarea_id=workarea_id,
        transition_id=transition_id,
        idempotency_key=idempotency_key,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    workarea_id: str,
    transition_id: str,
    *,
    client: AuthenticatedClient | Client,
    idempotency_key: str,
) -> ErrorEnvelope | Transition | None:
    """Ratify a pending transition; journalled with the policy snapshot.

     Requires: a second, non-proposing dreamer or admin (review.ratifier_is_proposer otherwise — R2
    extends to transitions). A held_by: service key is refused with access.verb_ceiling before any
    lookup (ADR-0025 d1).

    Args:
        workarea_id (str):
        transition_id (str):
        idempotency_key (str):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | Transition
    """

    return (
        await asyncio_detailed(
            workarea_id=workarea_id,
            transition_id=transition_id,
            client=client,
            idempotency_key=idempotency_key,
        )
    ).parsed
