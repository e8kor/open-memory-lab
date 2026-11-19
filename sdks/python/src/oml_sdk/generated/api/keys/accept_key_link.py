from http import HTTPStatus
from typing import Any
from urllib.parse import quote

import httpx

from ...client import AuthenticatedClient, Client
from ...models.error_envelope import ErrorEnvelope
from ...models.key_link import KeyLink
from ...types import UNSET, Response, Unset


def _get_kwargs(
    key: str,
    *,
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
        "url": "/v2/keys/{key}/accept_link".format(
            key=quote(str(key), safe=""),
        ),
    }

    _kwargs["headers"] = headers
    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> ErrorEnvelope | KeyLink:
    if response.status_code == 200:
        response_200 = KeyLink.from_dict(response.json())

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
) -> Response[ErrorEnvelope | KeyLink]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    key: str,
    *,
    client: AuthenticatedClient | Client,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
    idempotency_key: str,
) -> Response[ErrorEnvelope | KeyLink]:
    """Accept a link an admin made of a key to the signed-in person, so the key acts for them.

     Requires: the console session of the person the pending link names, through its capacity credential
    (the operator's decision of 2026-11-19: a link takes effect only once the named person accepts it
    from their own console session). A bearer key or any other caller that is not a console session is
    request.validation with details.field key and details.reason session_only; another person's session,
    the linking admin's included, is not_named_person; a key with no link pending is not_pending; a key
    revoked or past its expiry is revoked, and one no longer used_by person is not_used_by_person. A
    refusal journals nothing. Journalled as key.link_accepted; from then on the key acts for the person
    as ADR-0085 d16 says: it writes into their root wherever its grants allow, its fix and its
    settlement read the person, and oml-web may name it in OML-Acting-Key. No MCP tool or CLI command
    reaches the route. The Idempotency-Key is required and not replayed. Since v0.8.4.

    Args:
        key (str): A key, by its id or its name, resolved server-side like every reference; a
            revoked or expired key's name still resolves (ADR-0026 d4, d10).
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        idempotency_key (str):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | KeyLink]
    """

    kwargs = _get_kwargs(
        key=key,
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
    key: str,
    *,
    client: AuthenticatedClient | Client,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
    idempotency_key: str,
) -> ErrorEnvelope | KeyLink | None:
    """Accept a link an admin made of a key to the signed-in person, so the key acts for them.

     Requires: the console session of the person the pending link names, through its capacity credential
    (the operator's decision of 2026-11-19: a link takes effect only once the named person accepts it
    from their own console session). A bearer key or any other caller that is not a console session is
    request.validation with details.field key and details.reason session_only; another person's session,
    the linking admin's included, is not_named_person; a key with no link pending is not_pending; a key
    revoked or past its expiry is revoked, and one no longer used_by person is not_used_by_person. A
    refusal journals nothing. Journalled as key.link_accepted; from then on the key acts for the person
    as ADR-0085 d16 says: it writes into their root wherever its grants allow, its fix and its
    settlement read the person, and oml-web may name it in OML-Acting-Key. No MCP tool or CLI command
    reaches the route. The Idempotency-Key is required and not replayed. Since v0.8.4.

    Args:
        key (str): A key, by its id or its name, resolved server-side like every reference; a
            revoked or expired key's name still resolves (ADR-0026 d4, d10).
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        idempotency_key (str):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | KeyLink
    """

    return sync_detailed(
        key=key,
        client=client,
        x_request_id=x_request_id,
        x_correlation_id=x_correlation_id,
        traceparent=traceparent,
        idempotency_key=idempotency_key,
    ).parsed


async def asyncio_detailed(
    key: str,
    *,
    client: AuthenticatedClient | Client,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
    idempotency_key: str,
) -> Response[ErrorEnvelope | KeyLink]:
    """Accept a link an admin made of a key to the signed-in person, so the key acts for them.

     Requires: the console session of the person the pending link names, through its capacity credential
    (the operator's decision of 2026-11-19: a link takes effect only once the named person accepts it
    from their own console session). A bearer key or any other caller that is not a console session is
    request.validation with details.field key and details.reason session_only; another person's session,
    the linking admin's included, is not_named_person; a key with no link pending is not_pending; a key
    revoked or past its expiry is revoked, and one no longer used_by person is not_used_by_person. A
    refusal journals nothing. Journalled as key.link_accepted; from then on the key acts for the person
    as ADR-0085 d16 says: it writes into their root wherever its grants allow, its fix and its
    settlement read the person, and oml-web may name it in OML-Acting-Key. No MCP tool or CLI command
    reaches the route. The Idempotency-Key is required and not replayed. Since v0.8.4.

    Args:
        key (str): A key, by its id or its name, resolved server-side like every reference; a
            revoked or expired key's name still resolves (ADR-0026 d4, d10).
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        idempotency_key (str):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | KeyLink]
    """

    kwargs = _get_kwargs(
        key=key,
        x_request_id=x_request_id,
        x_correlation_id=x_correlation_id,
        traceparent=traceparent,
        idempotency_key=idempotency_key,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    key: str,
    *,
    client: AuthenticatedClient | Client,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
    idempotency_key: str,
) -> ErrorEnvelope | KeyLink | None:
    """Accept a link an admin made of a key to the signed-in person, so the key acts for them.

     Requires: the console session of the person the pending link names, through its capacity credential
    (the operator's decision of 2026-11-19: a link takes effect only once the named person accepts it
    from their own console session). A bearer key or any other caller that is not a console session is
    request.validation with details.field key and details.reason session_only; another person's session,
    the linking admin's included, is not_named_person; a key with no link pending is not_pending; a key
    revoked or past its expiry is revoked, and one no longer used_by person is not_used_by_person. A
    refusal journals nothing. Journalled as key.link_accepted; from then on the key acts for the person
    as ADR-0085 d16 says: it writes into their root wherever its grants allow, its fix and its
    settlement read the person, and oml-web may name it in OML-Acting-Key. No MCP tool or CLI command
    reaches the route. The Idempotency-Key is required and not replayed. Since v0.8.4.

    Args:
        key (str): A key, by its id or its name, resolved server-side like every reference; a
            revoked or expired key's name still resolves (ADR-0026 d4, d10).
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        idempotency_key (str):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | KeyLink
    """

    return (
        await asyncio_detailed(
            key=key,
            client=client,
            x_request_id=x_request_id,
            x_correlation_id=x_correlation_id,
            traceparent=traceparent,
            idempotency_key=idempotency_key,
        )
    ).parsed
