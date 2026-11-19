from http import HTTPStatus
from typing import Any
from urllib.parse import quote

import httpx

from ...client import AuthenticatedClient, Client
from ...models.error_envelope import ErrorEnvelope
from ...models.key_link import KeyLink
from ...models.key_link_request import KeyLinkRequest
from ...types import UNSET, Response, Unset


def _get_kwargs(
    key: str,
    *,
    body: KeyLinkRequest,
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
        "url": "/v2/keys/{key}/link".format(
            key=quote(str(key), safe=""),
        ),
    }

    _kwargs["json"] = body.to_dict()

    headers["Content-Type"] = "application/json"

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

    if response.status_code == 415:
        response_415 = ErrorEnvelope.from_dict(response.json())

        return response_415

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
    body: KeyLinkRequest,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
    idempotency_key: str,
) -> Response[ErrorEnvelope | KeyLink]:
    """Link a person's tenant key to the user who signs in, so the console acts with it in shared
    workspaces.

     Requires: a console admin session, <user>-admin through its capacity credential, in the key's own
    tenant; another role is access.verb_ceiling, a bearer admin key, which names no person, is
    request.validation with details.field key and details.reason session_admin_only, and no MCP tool or
    CLI command reaches the route (the operator, 2026-11-17: "Console only"). user names a user of the
    tenant with a user root and a web entry, none of it disabled, and never the calling admin's own
    person; otherwise request.validation with details.field /user and details.reason user_unknown,
    user_disabled or self_link. The key is a live tenant bearer key used_by person that holds no link;
    otherwise request.validation with details.field key and details.reason revoked (a key past its
    expiry too), already_linked (unlink it first), admin_key, not_a_tenant_key (a user key, a session
    key or a connector key) or not_used_by_person. Journalled as key.linked_person, with accepted false:
    the link is pending, and the key acts for no one, until that user accepts it from their own console
    session with POST /v2/keys/{key}/accept_link (the operator's decision of 2026-11-19). The key is
    refused already_linked while a link pends too. From the acceptance on, the key writes into that
    user's root as their key wherever its grants allow, and oml-web may name it in OML-Acting-Key beside
    that user's capacity credential on the console's acting routes (GET /v2/whoami, GET /v2/review/held,
    GET /v1/memories/{artifact_id}, POST /v2/review/verdicts/bulk): the request runs as the key, with
    its grants and verb ceilings. The header sent twice, on another route, without a capacity
    credential, or naming a key the link no longer admits answers auth.invalid with details.reason
    acting_key_not_linked and runs nothing (ADR-0085 d16, amended 2026-11-17). The Idempotency-Key is
    required and not replayed. Since v0.8.0.

    Args:
        key (str): A key, by its id or its name, resolved server-side like every reference; a
            revoked or expired key's name still resolves (ADR-0026 d4, d10).
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        idempotency_key (str):
        body (KeyLinkRequest):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | KeyLink]
    """

    kwargs = _get_kwargs(
        key=key,
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
    key: str,
    *,
    client: AuthenticatedClient | Client,
    body: KeyLinkRequest,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
    idempotency_key: str,
) -> ErrorEnvelope | KeyLink | None:
    """Link a person's tenant key to the user who signs in, so the console acts with it in shared
    workspaces.

     Requires: a console admin session, <user>-admin through its capacity credential, in the key's own
    tenant; another role is access.verb_ceiling, a bearer admin key, which names no person, is
    request.validation with details.field key and details.reason session_admin_only, and no MCP tool or
    CLI command reaches the route (the operator, 2026-11-17: "Console only"). user names a user of the
    tenant with a user root and a web entry, none of it disabled, and never the calling admin's own
    person; otherwise request.validation with details.field /user and details.reason user_unknown,
    user_disabled or self_link. The key is a live tenant bearer key used_by person that holds no link;
    otherwise request.validation with details.field key and details.reason revoked (a key past its
    expiry too), already_linked (unlink it first), admin_key, not_a_tenant_key (a user key, a session
    key or a connector key) or not_used_by_person. Journalled as key.linked_person, with accepted false:
    the link is pending, and the key acts for no one, until that user accepts it from their own console
    session with POST /v2/keys/{key}/accept_link (the operator's decision of 2026-11-19). The key is
    refused already_linked while a link pends too. From the acceptance on, the key writes into that
    user's root as their key wherever its grants allow, and oml-web may name it in OML-Acting-Key beside
    that user's capacity credential on the console's acting routes (GET /v2/whoami, GET /v2/review/held,
    GET /v1/memories/{artifact_id}, POST /v2/review/verdicts/bulk): the request runs as the key, with
    its grants and verb ceilings. The header sent twice, on another route, without a capacity
    credential, or naming a key the link no longer admits answers auth.invalid with details.reason
    acting_key_not_linked and runs nothing (ADR-0085 d16, amended 2026-11-17). The Idempotency-Key is
    required and not replayed. Since v0.8.0.

    Args:
        key (str): A key, by its id or its name, resolved server-side like every reference; a
            revoked or expired key's name still resolves (ADR-0026 d4, d10).
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        idempotency_key (str):
        body (KeyLinkRequest):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | KeyLink
    """

    return sync_detailed(
        key=key,
        client=client,
        body=body,
        x_request_id=x_request_id,
        x_correlation_id=x_correlation_id,
        traceparent=traceparent,
        idempotency_key=idempotency_key,
    ).parsed


async def asyncio_detailed(
    key: str,
    *,
    client: AuthenticatedClient | Client,
    body: KeyLinkRequest,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
    idempotency_key: str,
) -> Response[ErrorEnvelope | KeyLink]:
    """Link a person's tenant key to the user who signs in, so the console acts with it in shared
    workspaces.

     Requires: a console admin session, <user>-admin through its capacity credential, in the key's own
    tenant; another role is access.verb_ceiling, a bearer admin key, which names no person, is
    request.validation with details.field key and details.reason session_admin_only, and no MCP tool or
    CLI command reaches the route (the operator, 2026-11-17: "Console only"). user names a user of the
    tenant with a user root and a web entry, none of it disabled, and never the calling admin's own
    person; otherwise request.validation with details.field /user and details.reason user_unknown,
    user_disabled or self_link. The key is a live tenant bearer key used_by person that holds no link;
    otherwise request.validation with details.field key and details.reason revoked (a key past its
    expiry too), already_linked (unlink it first), admin_key, not_a_tenant_key (a user key, a session
    key or a connector key) or not_used_by_person. Journalled as key.linked_person, with accepted false:
    the link is pending, and the key acts for no one, until that user accepts it from their own console
    session with POST /v2/keys/{key}/accept_link (the operator's decision of 2026-11-19). The key is
    refused already_linked while a link pends too. From the acceptance on, the key writes into that
    user's root as their key wherever its grants allow, and oml-web may name it in OML-Acting-Key beside
    that user's capacity credential on the console's acting routes (GET /v2/whoami, GET /v2/review/held,
    GET /v1/memories/{artifact_id}, POST /v2/review/verdicts/bulk): the request runs as the key, with
    its grants and verb ceilings. The header sent twice, on another route, without a capacity
    credential, or naming a key the link no longer admits answers auth.invalid with details.reason
    acting_key_not_linked and runs nothing (ADR-0085 d16, amended 2026-11-17). The Idempotency-Key is
    required and not replayed. Since v0.8.0.

    Args:
        key (str): A key, by its id or its name, resolved server-side like every reference; a
            revoked or expired key's name still resolves (ADR-0026 d4, d10).
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        idempotency_key (str):
        body (KeyLinkRequest):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | KeyLink]
    """

    kwargs = _get_kwargs(
        key=key,
        body=body,
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
    body: KeyLinkRequest,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
    idempotency_key: str,
) -> ErrorEnvelope | KeyLink | None:
    """Link a person's tenant key to the user who signs in, so the console acts with it in shared
    workspaces.

     Requires: a console admin session, <user>-admin through its capacity credential, in the key's own
    tenant; another role is access.verb_ceiling, a bearer admin key, which names no person, is
    request.validation with details.field key and details.reason session_admin_only, and no MCP tool or
    CLI command reaches the route (the operator, 2026-11-17: "Console only"). user names a user of the
    tenant with a user root and a web entry, none of it disabled, and never the calling admin's own
    person; otherwise request.validation with details.field /user and details.reason user_unknown,
    user_disabled or self_link. The key is a live tenant bearer key used_by person that holds no link;
    otherwise request.validation with details.field key and details.reason revoked (a key past its
    expiry too), already_linked (unlink it first), admin_key, not_a_tenant_key (a user key, a session
    key or a connector key) or not_used_by_person. Journalled as key.linked_person, with accepted false:
    the link is pending, and the key acts for no one, until that user accepts it from their own console
    session with POST /v2/keys/{key}/accept_link (the operator's decision of 2026-11-19). The key is
    refused already_linked while a link pends too. From the acceptance on, the key writes into that
    user's root as their key wherever its grants allow, and oml-web may name it in OML-Acting-Key beside
    that user's capacity credential on the console's acting routes (GET /v2/whoami, GET /v2/review/held,
    GET /v1/memories/{artifact_id}, POST /v2/review/verdicts/bulk): the request runs as the key, with
    its grants and verb ceilings. The header sent twice, on another route, without a capacity
    credential, or naming a key the link no longer admits answers auth.invalid with details.reason
    acting_key_not_linked and runs nothing (ADR-0085 d16, amended 2026-11-17). The Idempotency-Key is
    required and not replayed. Since v0.8.0.

    Args:
        key (str): A key, by its id or its name, resolved server-side like every reference; a
            revoked or expired key's name still resolves (ADR-0026 d4, d10).
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        idempotency_key (str):
        body (KeyLinkRequest):

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
            body=body,
            x_request_id=x_request_id,
            x_correlation_id=x_correlation_id,
            traceparent=traceparent,
            idempotency_key=idempotency_key,
        )
    ).parsed
