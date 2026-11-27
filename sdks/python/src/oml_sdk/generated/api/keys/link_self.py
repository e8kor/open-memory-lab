from http import HTTPStatus
from typing import Any

import httpx

from ...client import AuthenticatedClient, Client
from ...models.error_envelope import ErrorEnvelope
from ...models.link_self_body import LinkSelfBody
from ...models.link_self_response_200 import LinkSelfResponse200
from ...types import UNSET, Response, Unset


def _get_kwargs(
    *,
    body: LinkSelfBody,
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
        "url": "/v2/keys/self/link",
    }

    _kwargs["json"] = body.to_dict()

    headers["Content-Type"] = "application/json"

    _kwargs["headers"] = headers
    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> ErrorEnvelope | LinkSelfResponse200:
    if response.status_code == 200:
        response_200 = LinkSelfResponse200.from_dict(response.json())

        return response_200

    if response.status_code == 403:
        response_403 = ErrorEnvelope.from_dict(response.json())

        return response_403

    response_default = ErrorEnvelope.from_dict(response.json())

    return response_default


def _build_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Response[ErrorEnvelope | LinkSelfResponse200]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    *,
    client: AuthenticatedClient | Client,
    body: LinkSelfBody,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> Response[ErrorEnvelope | LinkSelfResponse200]:
    """Link the calling admin key to its holder's person, pending that person's acceptance.

     Requires: an admin key presented by its own bearer value (possession); a session key is
    request.validation with details.field key and details.reason bearer_only, and another role
    access.verb_ceiling (ADR-0097 d22, decided by the operator, 2026-11-25). user names a user of the
    tenant with a user root and a web entry that is not disabled; otherwise details.field /user and
    details.reason user_unknown or user_disabled. The key is a live tenant bearer key used_by person
    that holds no link and none pending; otherwise details.field key and details.reason revoked,
    already_linked, not_a_tenant_key or not_used_by_person. The one self-link the server admits: POST
    /v2/keys/{key}/link keeps refusing self_link. Journalled as key.linked_person with actor self and
    accepted false: the key acts for no one until that person accepts the link from their own console
    session with POST /v2/keys/{key}/accept_link. A link is the link_key admin act, class consent,
    journalled first in the commit as admin.act with its reason, and key.linked_person names its act_id;
    reason is required and scanned, and a link without one is request.validation with details.field
    /reason and details.reason required (ADR-0097 d7, d26; amendment of 2026-11-27, answer 15). Since
    v0.8.6.

    Args:
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        body (LinkSelfBody):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | LinkSelfResponse200]
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
    body: LinkSelfBody,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> ErrorEnvelope | LinkSelfResponse200 | None:
    """Link the calling admin key to its holder's person, pending that person's acceptance.

     Requires: an admin key presented by its own bearer value (possession); a session key is
    request.validation with details.field key and details.reason bearer_only, and another role
    access.verb_ceiling (ADR-0097 d22, decided by the operator, 2026-11-25). user names a user of the
    tenant with a user root and a web entry that is not disabled; otherwise details.field /user and
    details.reason user_unknown or user_disabled. The key is a live tenant bearer key used_by person
    that holds no link and none pending; otherwise details.field key and details.reason revoked,
    already_linked, not_a_tenant_key or not_used_by_person. The one self-link the server admits: POST
    /v2/keys/{key}/link keeps refusing self_link. Journalled as key.linked_person with actor self and
    accepted false: the key acts for no one until that person accepts the link from their own console
    session with POST /v2/keys/{key}/accept_link. A link is the link_key admin act, class consent,
    journalled first in the commit as admin.act with its reason, and key.linked_person names its act_id;
    reason is required and scanned, and a link without one is request.validation with details.field
    /reason and details.reason required (ADR-0097 d7, d26; amendment of 2026-11-27, answer 15). Since
    v0.8.6.

    Args:
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        body (LinkSelfBody):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | LinkSelfResponse200
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
    body: LinkSelfBody,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> Response[ErrorEnvelope | LinkSelfResponse200]:
    """Link the calling admin key to its holder's person, pending that person's acceptance.

     Requires: an admin key presented by its own bearer value (possession); a session key is
    request.validation with details.field key and details.reason bearer_only, and another role
    access.verb_ceiling (ADR-0097 d22, decided by the operator, 2026-11-25). user names a user of the
    tenant with a user root and a web entry that is not disabled; otherwise details.field /user and
    details.reason user_unknown or user_disabled. The key is a live tenant bearer key used_by person
    that holds no link and none pending; otherwise details.field key and details.reason revoked,
    already_linked, not_a_tenant_key or not_used_by_person. The one self-link the server admits: POST
    /v2/keys/{key}/link keeps refusing self_link. Journalled as key.linked_person with actor self and
    accepted false: the key acts for no one until that person accepts the link from their own console
    session with POST /v2/keys/{key}/accept_link. A link is the link_key admin act, class consent,
    journalled first in the commit as admin.act with its reason, and key.linked_person names its act_id;
    reason is required and scanned, and a link without one is request.validation with details.field
    /reason and details.reason required (ADR-0097 d7, d26; amendment of 2026-11-27, answer 15). Since
    v0.8.6.

    Args:
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        body (LinkSelfBody):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | LinkSelfResponse200]
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
    body: LinkSelfBody,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> ErrorEnvelope | LinkSelfResponse200 | None:
    """Link the calling admin key to its holder's person, pending that person's acceptance.

     Requires: an admin key presented by its own bearer value (possession); a session key is
    request.validation with details.field key and details.reason bearer_only, and another role
    access.verb_ceiling (ADR-0097 d22, decided by the operator, 2026-11-25). user names a user of the
    tenant with a user root and a web entry that is not disabled; otherwise details.field /user and
    details.reason user_unknown or user_disabled. The key is a live tenant bearer key used_by person
    that holds no link and none pending; otherwise details.field key and details.reason revoked,
    already_linked, not_a_tenant_key or not_used_by_person. The one self-link the server admits: POST
    /v2/keys/{key}/link keeps refusing self_link. Journalled as key.linked_person with actor self and
    accepted false: the key acts for no one until that person accepts the link from their own console
    session with POST /v2/keys/{key}/accept_link. A link is the link_key admin act, class consent,
    journalled first in the commit as admin.act with its reason, and key.linked_person names its act_id;
    reason is required and scanned, and a link without one is request.validation with details.field
    /reason and details.reason required (ADR-0097 d7, d26; amendment of 2026-11-27, answer 15). Since
    v0.8.6.

    Args:
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        body (LinkSelfBody):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | LinkSelfResponse200
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
