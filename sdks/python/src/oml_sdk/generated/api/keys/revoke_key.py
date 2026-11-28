from http import HTTPStatus
from typing import Any
from urllib.parse import quote

import httpx

from ...client import AuthenticatedClient, Client
from ...models.error_envelope import ErrorEnvelope
from ...models.revoke_key_body import RevokeKeyBody
from ...models.revoke_key_response_200 import RevokeKeyResponse200
from ...types import UNSET, Response, Unset


def _get_kwargs(
    key: str,
    *,
    body: RevokeKeyBody | Unset = UNSET,
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
        "url": "/v2/keys/{key}/revoke".format(
            key=quote(str(key), safe=""),
        ),
    }

    if not isinstance(body, Unset):
        _kwargs["json"] = body.to_dict()

    headers["Content-Type"] = "application/json"

    _kwargs["headers"] = headers
    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> ErrorEnvelope | RevokeKeyResponse200:
    if response.status_code == 200:
        response_200 = RevokeKeyResponse200.from_dict(response.json())

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
) -> Response[ErrorEnvelope | RevokeKeyResponse200]:
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
    body: RevokeKeyBody | Unset = UNSET,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> Response[ErrorEnvelope | RevokeKeyResponse200]:
    """End one key, and nothing more.

     Requires: an admin key role in the key's tenant, a person's <user>-admin session key included, on
    any key of the tenant; or a person's <user>-web session key on that person's own keys, those that
    count as the person, any other key answering access.not_found (ADR-0097 d14, decided by the
    operator, 2026-11-25). Any other caller is access.verb_ceiling. The key authenticates nothing
    afterwards on any path: its value, its OAuth tokens and refresh tokens, and its connect codes, which
    are voided in the same commit. No cascade: the keys it issued keep working, and session keys come
    from the server, so revoking an admin key ends no sign-in. A session key (it ends with its entry),
    the server's reserved key and a revoked key are request.validation with details.field key and
    details.reason session_key, reserved or revoked. The body admits only reason, scanned, so cascade
    and rotation can be added later as new members; an unknown field is request.validation. An admin's
    revocation journals an admin.act, revoke_key (narrowing) or, for a key of another admin person,
    revoke_admin_key (against an admin), before key.revoked {key_id, revoked_by_key_id, actor_person,
    reason, act_id, cause}; a person's own revocation journals key.revoked alone, act_id null. Since
    v0.8.6.

    Args:
        key (str): A key, by its id or its name, resolved server-side like every reference; a
            revoked or expired key's name still resolves (ADR-0026 d4, d10).
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        body (RevokeKeyBody | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | RevokeKeyResponse200]
    """

    kwargs = _get_kwargs(
        key=key,
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
    key: str,
    *,
    client: AuthenticatedClient | Client,
    body: RevokeKeyBody | Unset = UNSET,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> ErrorEnvelope | RevokeKeyResponse200 | None:
    """End one key, and nothing more.

     Requires: an admin key role in the key's tenant, a person's <user>-admin session key included, on
    any key of the tenant; or a person's <user>-web session key on that person's own keys, those that
    count as the person, any other key answering access.not_found (ADR-0097 d14, decided by the
    operator, 2026-11-25). Any other caller is access.verb_ceiling. The key authenticates nothing
    afterwards on any path: its value, its OAuth tokens and refresh tokens, and its connect codes, which
    are voided in the same commit. No cascade: the keys it issued keep working, and session keys come
    from the server, so revoking an admin key ends no sign-in. A session key (it ends with its entry),
    the server's reserved key and a revoked key are request.validation with details.field key and
    details.reason session_key, reserved or revoked. The body admits only reason, scanned, so cascade
    and rotation can be added later as new members; an unknown field is request.validation. An admin's
    revocation journals an admin.act, revoke_key (narrowing) or, for a key of another admin person,
    revoke_admin_key (against an admin), before key.revoked {key_id, revoked_by_key_id, actor_person,
    reason, act_id, cause}; a person's own revocation journals key.revoked alone, act_id null. Since
    v0.8.6.

    Args:
        key (str): A key, by its id or its name, resolved server-side like every reference; a
            revoked or expired key's name still resolves (ADR-0026 d4, d10).
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        body (RevokeKeyBody | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | RevokeKeyResponse200
    """

    return sync_detailed(
        key=key,
        client=client,
        body=body,
        x_request_id=x_request_id,
        x_correlation_id=x_correlation_id,
        traceparent=traceparent,
    ).parsed


async def asyncio_detailed(
    key: str,
    *,
    client: AuthenticatedClient | Client,
    body: RevokeKeyBody | Unset = UNSET,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> Response[ErrorEnvelope | RevokeKeyResponse200]:
    """End one key, and nothing more.

     Requires: an admin key role in the key's tenant, a person's <user>-admin session key included, on
    any key of the tenant; or a person's <user>-web session key on that person's own keys, those that
    count as the person, any other key answering access.not_found (ADR-0097 d14, decided by the
    operator, 2026-11-25). Any other caller is access.verb_ceiling. The key authenticates nothing
    afterwards on any path: its value, its OAuth tokens and refresh tokens, and its connect codes, which
    are voided in the same commit. No cascade: the keys it issued keep working, and session keys come
    from the server, so revoking an admin key ends no sign-in. A session key (it ends with its entry),
    the server's reserved key and a revoked key are request.validation with details.field key and
    details.reason session_key, reserved or revoked. The body admits only reason, scanned, so cascade
    and rotation can be added later as new members; an unknown field is request.validation. An admin's
    revocation journals an admin.act, revoke_key (narrowing) or, for a key of another admin person,
    revoke_admin_key (against an admin), before key.revoked {key_id, revoked_by_key_id, actor_person,
    reason, act_id, cause}; a person's own revocation journals key.revoked alone, act_id null. Since
    v0.8.6.

    Args:
        key (str): A key, by its id or its name, resolved server-side like every reference; a
            revoked or expired key's name still resolves (ADR-0026 d4, d10).
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        body (RevokeKeyBody | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | RevokeKeyResponse200]
    """

    kwargs = _get_kwargs(
        key=key,
        body=body,
        x_request_id=x_request_id,
        x_correlation_id=x_correlation_id,
        traceparent=traceparent,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    key: str,
    *,
    client: AuthenticatedClient | Client,
    body: RevokeKeyBody | Unset = UNSET,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> ErrorEnvelope | RevokeKeyResponse200 | None:
    """End one key, and nothing more.

     Requires: an admin key role in the key's tenant, a person's <user>-admin session key included, on
    any key of the tenant; or a person's <user>-web session key on that person's own keys, those that
    count as the person, any other key answering access.not_found (ADR-0097 d14, decided by the
    operator, 2026-11-25). Any other caller is access.verb_ceiling. The key authenticates nothing
    afterwards on any path: its value, its OAuth tokens and refresh tokens, and its connect codes, which
    are voided in the same commit. No cascade: the keys it issued keep working, and session keys come
    from the server, so revoking an admin key ends no sign-in. A session key (it ends with its entry),
    the server's reserved key and a revoked key are request.validation with details.field key and
    details.reason session_key, reserved or revoked. The body admits only reason, scanned, so cascade
    and rotation can be added later as new members; an unknown field is request.validation. An admin's
    revocation journals an admin.act, revoke_key (narrowing) or, for a key of another admin person,
    revoke_admin_key (against an admin), before key.revoked {key_id, revoked_by_key_id, actor_person,
    reason, act_id, cause}; a person's own revocation journals key.revoked alone, act_id null. Since
    v0.8.6.

    Args:
        key (str): A key, by its id or its name, resolved server-side like every reference; a
            revoked or expired key's name still resolves (ADR-0026 d4, d10).
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        body (RevokeKeyBody | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | RevokeKeyResponse200
    """

    return (
        await asyncio_detailed(
            key=key,
            client=client,
            body=body,
            x_request_id=x_request_id,
            x_correlation_id=x_correlation_id,
            traceparent=traceparent,
        )
    ).parsed
