from http import HTTPStatus
from typing import Any, cast

import httpx

from ...client import AuthenticatedClient, Client
from ...models.error_envelope import ErrorEnvelope
from ...models.review_next_body import ReviewNextBody
from ...models.serve import Serve
from ...types import UNSET, Response, Unset


def _get_kwargs(
    *,
    body: ReviewNextBody | Unset = UNSET,
    idempotency_key: str,
) -> dict[str, Any]:
    headers: dict[str, Any] = {}
    headers["Idempotency-Key"] = idempotency_key

    _kwargs: dict[str, Any] = {
        "method": "post",
        "url": "/v2/review/next",
    }

    if not isinstance(body, Unset):
        _kwargs["json"] = body.to_dict()

    headers["Content-Type"] = "application/json"

    _kwargs["headers"] = headers
    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Any | ErrorEnvelope | Serve:
    if response.status_code == 200:
        response_200 = Serve.from_dict(response.json())

        return response_200

    if response.status_code == 204:
        response_204 = cast(Any, None)
        return response_204

    if response.status_code == 403:
        response_403 = ErrorEnvelope.from_dict(response.json())

        return response_403

    if response.status_code == 415:
        response_415 = ErrorEnvelope.from_dict(response.json())

        return response_415

    if response.status_code == 429:
        response_429 = ErrorEnvelope.from_dict(response.json())

        return response_429

    response_default = ErrorEnvelope.from_dict(response.json())

    return response_default


def _build_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Response[Any | ErrorEnvelope | Serve]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    *,
    client: AuthenticatedClient | Client,
    body: ReviewNextBody | Unset = UNSET,
    idempotency_key: str,
) -> Response[Any | ErrorEnvelope | Serve]:
    """Serve exactly one held item — body, ALL attachments, the replaced version for a supersession — under
    an expiring serve_id.

     Requires: dreamer(review). Serves only rows within the key's dreamer effective access on the row's
    workarea; 204 when nothing in scope waits. A held_by: service key is refused with
    access.verb_ceiling before any lookup, as on every review-class verb and status-transition ratify
    (ADR-0025 d1). Serve bound server-side to (reviewer key, artifact version, attachment digest set);
    refused per item on lineage collision (review.author_in_lineage).

    Args:
        idempotency_key (str):
        body (ReviewNextBody | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Any | ErrorEnvelope | Serve]
    """

    kwargs = _get_kwargs(
        body=body,
        idempotency_key=idempotency_key,
    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)


def sync(
    *,
    client: AuthenticatedClient | Client,
    body: ReviewNextBody | Unset = UNSET,
    idempotency_key: str,
) -> Any | ErrorEnvelope | Serve | None:
    """Serve exactly one held item — body, ALL attachments, the replaced version for a supersession — under
    an expiring serve_id.

     Requires: dreamer(review). Serves only rows within the key's dreamer effective access on the row's
    workarea; 204 when nothing in scope waits. A held_by: service key is refused with
    access.verb_ceiling before any lookup, as on every review-class verb and status-transition ratify
    (ADR-0025 d1). Serve bound server-side to (reviewer key, artifact version, attachment digest set);
    refused per item on lineage collision (review.author_in_lineage).

    Args:
        idempotency_key (str):
        body (ReviewNextBody | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Any | ErrorEnvelope | Serve
    """

    return sync_detailed(
        client=client,
        body=body,
        idempotency_key=idempotency_key,
    ).parsed


async def asyncio_detailed(
    *,
    client: AuthenticatedClient | Client,
    body: ReviewNextBody | Unset = UNSET,
    idempotency_key: str,
) -> Response[Any | ErrorEnvelope | Serve]:
    """Serve exactly one held item — body, ALL attachments, the replaced version for a supersession — under
    an expiring serve_id.

     Requires: dreamer(review). Serves only rows within the key's dreamer effective access on the row's
    workarea; 204 when nothing in scope waits. A held_by: service key is refused with
    access.verb_ceiling before any lookup, as on every review-class verb and status-transition ratify
    (ADR-0025 d1). Serve bound server-side to (reviewer key, artifact version, attachment digest set);
    refused per item on lineage collision (review.author_in_lineage).

    Args:
        idempotency_key (str):
        body (ReviewNextBody | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Any | ErrorEnvelope | Serve]
    """

    kwargs = _get_kwargs(
        body=body,
        idempotency_key=idempotency_key,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    *,
    client: AuthenticatedClient | Client,
    body: ReviewNextBody | Unset = UNSET,
    idempotency_key: str,
) -> Any | ErrorEnvelope | Serve | None:
    """Serve exactly one held item — body, ALL attachments, the replaced version for a supersession — under
    an expiring serve_id.

     Requires: dreamer(review). Serves only rows within the key's dreamer effective access on the row's
    workarea; 204 when nothing in scope waits. A held_by: service key is refused with
    access.verb_ceiling before any lookup, as on every review-class verb and status-transition ratify
    (ADR-0025 d1). Serve bound server-side to (reviewer key, artifact version, attachment digest set);
    refused per item on lineage collision (review.author_in_lineage).

    Args:
        idempotency_key (str):
        body (ReviewNextBody | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Any | ErrorEnvelope | Serve
    """

    return (
        await asyncio_detailed(
            client=client,
            body=body,
            idempotency_key=idempotency_key,
        )
    ).parsed
