from http import HTTPStatus
from typing import Any, cast

import httpx

from ... import errors
from ...client import AuthenticatedClient, Client
from ...models.authorize_submit_body_type_0 import AuthorizeSubmitBodyType0
from ...models.authorize_submit_body_type_1 import AuthorizeSubmitBodyType1
from ...types import UNSET, Response, Unset


def _get_kwargs(
    *,
    body: AuthorizeSubmitBodyType0 | AuthorizeSubmitBodyType1,
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
        "url": "/oauth/authorize",
    }

    _kwargs["data"] = body.to_dict()
    headers["Content-Type"] = "application/x-www-form-urlencoded"

    _kwargs["headers"] = headers
    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Any | str | None:
    if response.status_code == 200:
        response_200 = response.text
        return response_200

    if response.status_code == 303:
        response_303 = cast(Any, None)
        return response_303

    if response.status_code == 400:
        response_400 = response.text
        return response_400

    if response.status_code == 403:
        response_403 = response.text
        return response_403

    if client.raise_on_unexpected_status:
        raise errors.UnexpectedStatus(response.status_code, response.content)
    else:
        return None


def _build_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Response[Any | str]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    *,
    client: AuthenticatedClient | Client,
    body: AuthorizeSubmitBodyType0 | AuthorizeSubmitBodyType1,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> Response[Any | str]:
    """The page's own form — the connect code, then the confirmation.

     Two submissions and no others. Every submission first needs either Sec-Fetch-Site: same-origin or,
    when that header is absent, an Origin equal to the issuer origin; Origin: null and anything else
    answer 403 oauth.origin_refused on the page, never redirected, consuming no attempt. Step 2 carries
    exactly handle (the sealed handle of the first page) and connect_code; step 3 carries exactly handle
    and action, where the handle names the request row a verified code wrote and whose code is not
    voided (docs/architecture/roles-and-keys.md, Connections); any other combination is
    request.validation on the page. With a correct connect_code of a key that is neither revoked nor
    expired, the code is spent, the authorization request's row is written under a fresh handle, and the
    page shows the tenant by its operator-set name, the key's name, principal, role and grants and what
    the connection may do, with Connect and Cancel; a wrong code answers the form again (400
    oauth.connect_code_invalid) with the attempts left and a new sealed handle, and the fifth wrong
    entry redirects with access_denied. action=connect creates the connection (authorized) and redirects
    with a 60-second, single-use authorization code, state and iss; action=cancel redirects with
    access_denied, state and iss. Connect and Cancel each delete the authorization request's row in
    their own commit, so a later step-3 POST with that handle names no row and answers
    oauth.authorization_expired on the page (ADR-0031 d9).

    Args:
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        body (AuthorizeSubmitBodyType0 | AuthorizeSubmitBodyType1):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Any | str]
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
    body: AuthorizeSubmitBodyType0 | AuthorizeSubmitBodyType1,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> Any | str | None:
    """The page's own form — the connect code, then the confirmation.

     Two submissions and no others. Every submission first needs either Sec-Fetch-Site: same-origin or,
    when that header is absent, an Origin equal to the issuer origin; Origin: null and anything else
    answer 403 oauth.origin_refused on the page, never redirected, consuming no attempt. Step 2 carries
    exactly handle (the sealed handle of the first page) and connect_code; step 3 carries exactly handle
    and action, where the handle names the request row a verified code wrote and whose code is not
    voided (docs/architecture/roles-and-keys.md, Connections); any other combination is
    request.validation on the page. With a correct connect_code of a key that is neither revoked nor
    expired, the code is spent, the authorization request's row is written under a fresh handle, and the
    page shows the tenant by its operator-set name, the key's name, principal, role and grants and what
    the connection may do, with Connect and Cancel; a wrong code answers the form again (400
    oauth.connect_code_invalid) with the attempts left and a new sealed handle, and the fifth wrong
    entry redirects with access_denied. action=connect creates the connection (authorized) and redirects
    with a 60-second, single-use authorization code, state and iss; action=cancel redirects with
    access_denied, state and iss. Connect and Cancel each delete the authorization request's row in
    their own commit, so a later step-3 POST with that handle names no row and answers
    oauth.authorization_expired on the page (ADR-0031 d9).

    Args:
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        body (AuthorizeSubmitBodyType0 | AuthorizeSubmitBodyType1):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Any | str
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
    body: AuthorizeSubmitBodyType0 | AuthorizeSubmitBodyType1,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> Response[Any | str]:
    """The page's own form — the connect code, then the confirmation.

     Two submissions and no others. Every submission first needs either Sec-Fetch-Site: same-origin or,
    when that header is absent, an Origin equal to the issuer origin; Origin: null and anything else
    answer 403 oauth.origin_refused on the page, never redirected, consuming no attempt. Step 2 carries
    exactly handle (the sealed handle of the first page) and connect_code; step 3 carries exactly handle
    and action, where the handle names the request row a verified code wrote and whose code is not
    voided (docs/architecture/roles-and-keys.md, Connections); any other combination is
    request.validation on the page. With a correct connect_code of a key that is neither revoked nor
    expired, the code is spent, the authorization request's row is written under a fresh handle, and the
    page shows the tenant by its operator-set name, the key's name, principal, role and grants and what
    the connection may do, with Connect and Cancel; a wrong code answers the form again (400
    oauth.connect_code_invalid) with the attempts left and a new sealed handle, and the fifth wrong
    entry redirects with access_denied. action=connect creates the connection (authorized) and redirects
    with a 60-second, single-use authorization code, state and iss; action=cancel redirects with
    access_denied, state and iss. Connect and Cancel each delete the authorization request's row in
    their own commit, so a later step-3 POST with that handle names no row and answers
    oauth.authorization_expired on the page (ADR-0031 d9).

    Args:
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        body (AuthorizeSubmitBodyType0 | AuthorizeSubmitBodyType1):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[Any | str]
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
    body: AuthorizeSubmitBodyType0 | AuthorizeSubmitBodyType1,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> Any | str | None:
    """The page's own form — the connect code, then the confirmation.

     Two submissions and no others. Every submission first needs either Sec-Fetch-Site: same-origin or,
    when that header is absent, an Origin equal to the issuer origin; Origin: null and anything else
    answer 403 oauth.origin_refused on the page, never redirected, consuming no attempt. Step 2 carries
    exactly handle (the sealed handle of the first page) and connect_code; step 3 carries exactly handle
    and action, where the handle names the request row a verified code wrote and whose code is not
    voided (docs/architecture/roles-and-keys.md, Connections); any other combination is
    request.validation on the page. With a correct connect_code of a key that is neither revoked nor
    expired, the code is spent, the authorization request's row is written under a fresh handle, and the
    page shows the tenant by its operator-set name, the key's name, principal, role and grants and what
    the connection may do, with Connect and Cancel; a wrong code answers the form again (400
    oauth.connect_code_invalid) with the attempts left and a new sealed handle, and the fifth wrong
    entry redirects with access_denied. action=connect creates the connection (authorized) and redirects
    with a 60-second, single-use authorization code, state and iss; action=cancel redirects with
    access_denied, state and iss. Connect and Cancel each delete the authorization request's row in
    their own commit, so a later step-3 POST with that handle names no row and answers
    oauth.authorization_expired on the page (ADR-0031 d9).

    Args:
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        body (AuthorizeSubmitBodyType0 | AuthorizeSubmitBodyType1):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Any | str
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
