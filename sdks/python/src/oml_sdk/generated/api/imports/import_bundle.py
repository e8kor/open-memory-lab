from http import HTTPStatus
from typing import Any

import httpx

from ...client import AuthenticatedClient, Client
from ...models.error_envelope import ErrorEnvelope
from ...models.import_answer import ImportAnswer
from ...models.import_request import ImportRequest
from ...models.import_status import ImportStatus
from ...types import UNSET, Response, Unset


def _get_kwargs(
    *,
    body: ImportRequest,
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
        "url": "/v2/imports",
    }

    _kwargs["json"] = body.to_dict()

    headers["Content-Type"] = "application/json"

    _kwargs["headers"] = headers
    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> ErrorEnvelope | ImportAnswer | ImportStatus:
    if response.status_code == 200:
        response_200 = ImportAnswer.from_dict(response.json())

        return response_200

    if response.status_code == 202:
        response_202 = ImportStatus.from_dict(response.json())

        return response_202

    if response.status_code == 400:
        response_400 = ErrorEnvelope.from_dict(response.json())

        return response_400

    if response.status_code == 403:
        response_403 = ErrorEnvelope.from_dict(response.json())

        return response_403

    if response.status_code == 404:
        response_404 = ErrorEnvelope.from_dict(response.json())

        return response_404

    if response.status_code == 409:
        response_409 = ErrorEnvelope.from_dict(response.json())

        return response_409

    if response.status_code == 410:
        response_410 = ErrorEnvelope.from_dict(response.json())

        return response_410

    if response.status_code == 413:
        response_413 = ErrorEnvelope.from_dict(response.json())

        return response_413

    if response.status_code == 415:
        response_415 = ErrorEnvelope.from_dict(response.json())

        return response_415

    if response.status_code == 422:
        response_422 = ErrorEnvelope.from_dict(response.json())

        return response_422

    if response.status_code == 429:
        response_429 = ErrorEnvelope.from_dict(response.json())

        return response_429

    if response.status_code == 503:
        response_503 = ErrorEnvelope.from_dict(response.json())

        return response_503

    response_default = ErrorEnvelope.from_dict(response.json())

    return response_default


def _build_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Response[ErrorEnvelope | ImportAnswer | ImportStatus]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    *,
    client: AuthenticatedClient | Client,
    body: ImportRequest,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> Response[ErrorEnvelope | ImportAnswer | ImportStatus]:
    """Plan or run an import of a completed upload into an in-progress workspace; every version lands held.

     Requires: a writer key used by a person; any other key is access.verb_ceiling with details.reason
    import_person_only (ADR-0084 d11). upload_id is a completed upload session of purpose import that
    this key opened (ADR-0004 d3), holding the decrypted bundle; another key's is access.not_found, one
    in another state upload.session_invalid, one past its expiry 410 upload.expired. target is a
    development workspace in progress the key writes, or a bare name this key may create, which the
    import creates in progress; any other target is 409 import.target_not_in_progress, and a taken name
    name.taken (ADR-0084 d6, d9). The bundle is verified before anything is written, each check under
    its own bundle.* code (d7). expect_key is the source key's fingerprint, 64 hex digits; the first
    import from a source requires it (request.validation, details.field /expect_key, details.reason
    first_import) and pins it, and a later bundle under another key, or with none named, is 409
    import.source_key_changed (R11). A bundle older than the last imported from its source is 409
    import.bundle_older unless allow_older (d8). A plan that does not fit the account's meters is 429
    import.over_quota and writes nothing. dry_run answers the plan and binds nothing. Otherwise the
    upload binds once, each claim key is written as the plan says, held by this key with the source's
    origin kind and its import_provenance, under the idempotency key import:<target>:<source
    server>:<source tenant>:<artifact>:<version>, so the same bundle again writes nothing; a version the
    target's scan or rules refuse is skipped and named in refusals. A claim edge whose two ends the
    target carries is proposed by this key where its verb ceiling allows, which a writer key's does not;
    every edge not proposed is counted in edges_reported, and a refused proposal is named in refusals as
    edge:<src>:<dst>. One import or dry run per tenant runs at a time, and two in the server; another is
    409 import.busy, retryable. A dry run answers 200 with the plan. Otherwise the upload binds, the
    source key is pinned, and the answer is 202 with the import running; the run goes on after the
    answer, and GET /v2/imports/{id} answers its state until it is done or failed. The import journals
    workspace.imported, listing each version it wrote, and the upload's key is nulled when it ends; an
    import running for more than an hour that the server is not running ends failed at the next sweep.
    No Idempotency-Key is taken: the upload binds once. Since v0.8.2.

    Args:
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        body (ImportRequest): An import of a completed upload (ADR-0084 d15, I4).

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | ImportAnswer | ImportStatus]
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
    body: ImportRequest,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> ErrorEnvelope | ImportAnswer | ImportStatus | None:
    """Plan or run an import of a completed upload into an in-progress workspace; every version lands held.

     Requires: a writer key used by a person; any other key is access.verb_ceiling with details.reason
    import_person_only (ADR-0084 d11). upload_id is a completed upload session of purpose import that
    this key opened (ADR-0004 d3), holding the decrypted bundle; another key's is access.not_found, one
    in another state upload.session_invalid, one past its expiry 410 upload.expired. target is a
    development workspace in progress the key writes, or a bare name this key may create, which the
    import creates in progress; any other target is 409 import.target_not_in_progress, and a taken name
    name.taken (ADR-0084 d6, d9). The bundle is verified before anything is written, each check under
    its own bundle.* code (d7). expect_key is the source key's fingerprint, 64 hex digits; the first
    import from a source requires it (request.validation, details.field /expect_key, details.reason
    first_import) and pins it, and a later bundle under another key, or with none named, is 409
    import.source_key_changed (R11). A bundle older than the last imported from its source is 409
    import.bundle_older unless allow_older (d8). A plan that does not fit the account's meters is 429
    import.over_quota and writes nothing. dry_run answers the plan and binds nothing. Otherwise the
    upload binds once, each claim key is written as the plan says, held by this key with the source's
    origin kind and its import_provenance, under the idempotency key import:<target>:<source
    server>:<source tenant>:<artifact>:<version>, so the same bundle again writes nothing; a version the
    target's scan or rules refuse is skipped and named in refusals. A claim edge whose two ends the
    target carries is proposed by this key where its verb ceiling allows, which a writer key's does not;
    every edge not proposed is counted in edges_reported, and a refused proposal is named in refusals as
    edge:<src>:<dst>. One import or dry run per tenant runs at a time, and two in the server; another is
    409 import.busy, retryable. A dry run answers 200 with the plan. Otherwise the upload binds, the
    source key is pinned, and the answer is 202 with the import running; the run goes on after the
    answer, and GET /v2/imports/{id} answers its state until it is done or failed. The import journals
    workspace.imported, listing each version it wrote, and the upload's key is nulled when it ends; an
    import running for more than an hour that the server is not running ends failed at the next sweep.
    No Idempotency-Key is taken: the upload binds once. Since v0.8.2.

    Args:
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        body (ImportRequest): An import of a completed upload (ADR-0084 d15, I4).

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | ImportAnswer | ImportStatus
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
    body: ImportRequest,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> Response[ErrorEnvelope | ImportAnswer | ImportStatus]:
    """Plan or run an import of a completed upload into an in-progress workspace; every version lands held.

     Requires: a writer key used by a person; any other key is access.verb_ceiling with details.reason
    import_person_only (ADR-0084 d11). upload_id is a completed upload session of purpose import that
    this key opened (ADR-0004 d3), holding the decrypted bundle; another key's is access.not_found, one
    in another state upload.session_invalid, one past its expiry 410 upload.expired. target is a
    development workspace in progress the key writes, or a bare name this key may create, which the
    import creates in progress; any other target is 409 import.target_not_in_progress, and a taken name
    name.taken (ADR-0084 d6, d9). The bundle is verified before anything is written, each check under
    its own bundle.* code (d7). expect_key is the source key's fingerprint, 64 hex digits; the first
    import from a source requires it (request.validation, details.field /expect_key, details.reason
    first_import) and pins it, and a later bundle under another key, or with none named, is 409
    import.source_key_changed (R11). A bundle older than the last imported from its source is 409
    import.bundle_older unless allow_older (d8). A plan that does not fit the account's meters is 429
    import.over_quota and writes nothing. dry_run answers the plan and binds nothing. Otherwise the
    upload binds once, each claim key is written as the plan says, held by this key with the source's
    origin kind and its import_provenance, under the idempotency key import:<target>:<source
    server>:<source tenant>:<artifact>:<version>, so the same bundle again writes nothing; a version the
    target's scan or rules refuse is skipped and named in refusals. A claim edge whose two ends the
    target carries is proposed by this key where its verb ceiling allows, which a writer key's does not;
    every edge not proposed is counted in edges_reported, and a refused proposal is named in refusals as
    edge:<src>:<dst>. One import or dry run per tenant runs at a time, and two in the server; another is
    409 import.busy, retryable. A dry run answers 200 with the plan. Otherwise the upload binds, the
    source key is pinned, and the answer is 202 with the import running; the run goes on after the
    answer, and GET /v2/imports/{id} answers its state until it is done or failed. The import journals
    workspace.imported, listing each version it wrote, and the upload's key is nulled when it ends; an
    import running for more than an hour that the server is not running ends failed at the next sweep.
    No Idempotency-Key is taken: the upload binds once. Since v0.8.2.

    Args:
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        body (ImportRequest): An import of a completed upload (ADR-0084 d15, I4).

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | ImportAnswer | ImportStatus]
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
    body: ImportRequest,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> ErrorEnvelope | ImportAnswer | ImportStatus | None:
    """Plan or run an import of a completed upload into an in-progress workspace; every version lands held.

     Requires: a writer key used by a person; any other key is access.verb_ceiling with details.reason
    import_person_only (ADR-0084 d11). upload_id is a completed upload session of purpose import that
    this key opened (ADR-0004 d3), holding the decrypted bundle; another key's is access.not_found, one
    in another state upload.session_invalid, one past its expiry 410 upload.expired. target is a
    development workspace in progress the key writes, or a bare name this key may create, which the
    import creates in progress; any other target is 409 import.target_not_in_progress, and a taken name
    name.taken (ADR-0084 d6, d9). The bundle is verified before anything is written, each check under
    its own bundle.* code (d7). expect_key is the source key's fingerprint, 64 hex digits; the first
    import from a source requires it (request.validation, details.field /expect_key, details.reason
    first_import) and pins it, and a later bundle under another key, or with none named, is 409
    import.source_key_changed (R11). A bundle older than the last imported from its source is 409
    import.bundle_older unless allow_older (d8). A plan that does not fit the account's meters is 429
    import.over_quota and writes nothing. dry_run answers the plan and binds nothing. Otherwise the
    upload binds once, each claim key is written as the plan says, held by this key with the source's
    origin kind and its import_provenance, under the idempotency key import:<target>:<source
    server>:<source tenant>:<artifact>:<version>, so the same bundle again writes nothing; a version the
    target's scan or rules refuse is skipped and named in refusals. A claim edge whose two ends the
    target carries is proposed by this key where its verb ceiling allows, which a writer key's does not;
    every edge not proposed is counted in edges_reported, and a refused proposal is named in refusals as
    edge:<src>:<dst>. One import or dry run per tenant runs at a time, and two in the server; another is
    409 import.busy, retryable. A dry run answers 200 with the plan. Otherwise the upload binds, the
    source key is pinned, and the answer is 202 with the import running; the run goes on after the
    answer, and GET /v2/imports/{id} answers its state until it is done or failed. The import journals
    workspace.imported, listing each version it wrote, and the upload's key is nulled when it ends; an
    import running for more than an hour that the server is not running ends failed at the next sweep.
    No Idempotency-Key is taken: the upload binds once. Since v0.8.2.

    Args:
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        body (ImportRequest): An import of a completed upload (ADR-0084 d15, I4).

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | ImportAnswer | ImportStatus
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
