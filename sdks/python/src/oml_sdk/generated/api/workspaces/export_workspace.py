from http import HTTPStatus
from io import BytesIO
from typing import Any
from urllib.parse import quote

import httpx

from ...client import AuthenticatedClient, Client
from ...models.error_envelope import ErrorEnvelope
from ...models.export_request import ExportRequest
from ...types import UNSET, File, Response, Unset


def _get_kwargs(
    workspace: str,
    *,
    body: ExportRequest,
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
        "url": "/v2/workspaces/{workspace}/exports".format(
            workspace=quote(str(workspace), safe=""),
        ),
    }

    _kwargs["json"] = body.to_dict()

    headers["Content-Type"] = "application/json"

    _kwargs["headers"] = headers
    return _kwargs


def _parse_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> ErrorEnvelope | File:
    if response.status_code == 200:
        response_200 = File(payload=BytesIO(response.content))

        return response_200

    if response.status_code == 415:
        response_415 = ErrorEnvelope.from_dict(response.json())

        return response_415

    response_default = ErrorEnvelope.from_dict(response.json())

    return response_default


def _build_response(
    *, client: AuthenticatedClient | Client, response: httpx.Response
) -> Response[ErrorEnvelope | File]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    workspace: str,
    *,
    client: AuthenticatedClient | Client,
    body: ExportRequest,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> Response[ErrorEnvelope | File]:
    """A person exports a workspace's subtree as a signed bundle encrypted to named age recipients
    (ADR-0084).

     Requires: a key used_by person that is not an admin key and carries no profile, with reader
    effective access on every node of the subtree. Any other key is refused before any lookup with
    access.verb_ceiling, details.reason export_person_only. No recipient, or one that is not an age
    X25519 public key, is bundle.unencrypted, checked before anything is read. The answer is the bundle,
    format oml-bundle/1, as a binary age file (docs/data/bundle-format.md). history mode carries every
    version ever promoted and the exporter's own held, rejected, demoted and withdrawn versions, never
    another key's; an erased version travels as a marker. mode release, release and since are refused
    request.validation, details.reason releases_unbuilt, until releases are built; history is the
    default in a user root and must be named elsewhere. The tenant's export policy (own_root by default)
    refuses with export.disabled; a subtree node the key cannot read refuses the whole export with
    export.not_readable. Journals workspace.exported with the manifest digest, the counts and the
    recipients' fingerprints, and counts the bytes on the exports meter. Each call exports again under a
    new salt, so it takes no Idempotency-Key. Since v0.8.2.

    Args:
        workspace (str): A workspace, by its node id, its name or its qualified name. Resolved
            server-side before any check that reads state; an unknown name answers exactly what an
            unknown id answers. A user key resolves a bare name in its own user root's namespace, plus
            universe and its root's name; every other key in the shared namespace (ADR-0044 d5). A
            derived path, a value of three or more labels or one whose first label is universe, is
            neither, and is refused (request.validation).
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        body (ExportRequest):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | File]
    """

    kwargs = _get_kwargs(
        workspace=workspace,
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
    workspace: str,
    *,
    client: AuthenticatedClient | Client,
    body: ExportRequest,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> ErrorEnvelope | File | None:
    """A person exports a workspace's subtree as a signed bundle encrypted to named age recipients
    (ADR-0084).

     Requires: a key used_by person that is not an admin key and carries no profile, with reader
    effective access on every node of the subtree. Any other key is refused before any lookup with
    access.verb_ceiling, details.reason export_person_only. No recipient, or one that is not an age
    X25519 public key, is bundle.unencrypted, checked before anything is read. The answer is the bundle,
    format oml-bundle/1, as a binary age file (docs/data/bundle-format.md). history mode carries every
    version ever promoted and the exporter's own held, rejected, demoted and withdrawn versions, never
    another key's; an erased version travels as a marker. mode release, release and since are refused
    request.validation, details.reason releases_unbuilt, until releases are built; history is the
    default in a user root and must be named elsewhere. The tenant's export policy (own_root by default)
    refuses with export.disabled; a subtree node the key cannot read refuses the whole export with
    export.not_readable. Journals workspace.exported with the manifest digest, the counts and the
    recipients' fingerprints, and counts the bytes on the exports meter. Each call exports again under a
    new salt, so it takes no Idempotency-Key. Since v0.8.2.

    Args:
        workspace (str): A workspace, by its node id, its name or its qualified name. Resolved
            server-side before any check that reads state; an unknown name answers exactly what an
            unknown id answers. A user key resolves a bare name in its own user root's namespace, plus
            universe and its root's name; every other key in the shared namespace (ADR-0044 d5). A
            derived path, a value of three or more labels or one whose first label is universe, is
            neither, and is refused (request.validation).
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        body (ExportRequest):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | File
    """

    return sync_detailed(
        workspace=workspace,
        client=client,
        body=body,
        x_request_id=x_request_id,
        x_correlation_id=x_correlation_id,
        traceparent=traceparent,
    ).parsed


async def asyncio_detailed(
    workspace: str,
    *,
    client: AuthenticatedClient | Client,
    body: ExportRequest,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> Response[ErrorEnvelope | File]:
    """A person exports a workspace's subtree as a signed bundle encrypted to named age recipients
    (ADR-0084).

     Requires: a key used_by person that is not an admin key and carries no profile, with reader
    effective access on every node of the subtree. Any other key is refused before any lookup with
    access.verb_ceiling, details.reason export_person_only. No recipient, or one that is not an age
    X25519 public key, is bundle.unencrypted, checked before anything is read. The answer is the bundle,
    format oml-bundle/1, as a binary age file (docs/data/bundle-format.md). history mode carries every
    version ever promoted and the exporter's own held, rejected, demoted and withdrawn versions, never
    another key's; an erased version travels as a marker. mode release, release and since are refused
    request.validation, details.reason releases_unbuilt, until releases are built; history is the
    default in a user root and must be named elsewhere. The tenant's export policy (own_root by default)
    refuses with export.disabled; a subtree node the key cannot read refuses the whole export with
    export.not_readable. Journals workspace.exported with the manifest digest, the counts and the
    recipients' fingerprints, and counts the bytes on the exports meter. Each call exports again under a
    new salt, so it takes no Idempotency-Key. Since v0.8.2.

    Args:
        workspace (str): A workspace, by its node id, its name or its qualified name. Resolved
            server-side before any check that reads state; an unknown name answers exactly what an
            unknown id answers. A user key resolves a bare name in its own user root's namespace, plus
            universe and its root's name; every other key in the shared namespace (ADR-0044 d5). A
            derived path, a value of three or more labels or one whose first label is universe, is
            neither, and is refused (request.validation).
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        body (ExportRequest):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[ErrorEnvelope | File]
    """

    kwargs = _get_kwargs(
        workspace=workspace,
        body=body,
        x_request_id=x_request_id,
        x_correlation_id=x_correlation_id,
        traceparent=traceparent,
    )

    response = await client.get_async_httpx_client().request(**kwargs)

    return _build_response(client=client, response=response)


async def asyncio(
    workspace: str,
    *,
    client: AuthenticatedClient | Client,
    body: ExportRequest,
    x_request_id: str | Unset = UNSET,
    x_correlation_id: str | Unset = UNSET,
    traceparent: str | Unset = UNSET,
) -> ErrorEnvelope | File | None:
    """A person exports a workspace's subtree as a signed bundle encrypted to named age recipients
    (ADR-0084).

     Requires: a key used_by person that is not an admin key and carries no profile, with reader
    effective access on every node of the subtree. Any other key is refused before any lookup with
    access.verb_ceiling, details.reason export_person_only. No recipient, or one that is not an age
    X25519 public key, is bundle.unencrypted, checked before anything is read. The answer is the bundle,
    format oml-bundle/1, as a binary age file (docs/data/bundle-format.md). history mode carries every
    version ever promoted and the exporter's own held, rejected, demoted and withdrawn versions, never
    another key's; an erased version travels as a marker. mode release, release and since are refused
    request.validation, details.reason releases_unbuilt, until releases are built; history is the
    default in a user root and must be named elsewhere. The tenant's export policy (own_root by default)
    refuses with export.disabled; a subtree node the key cannot read refuses the whole export with
    export.not_readable. Journals workspace.exported with the manifest digest, the counts and the
    recipients' fingerprints, and counts the bytes on the exports meter. Each call exports again under a
    new salt, so it takes no Idempotency-Key. Since v0.8.2.

    Args:
        workspace (str): A workspace, by its node id, its name or its qualified name. Resolved
            server-side before any check that reads state; an unknown name answers exactly what an
            unknown id answers. A user key resolves a bare name in its own user root's namespace, plus
            universe and its root's name; every other key in the shared namespace (ADR-0044 d5). A
            derived path, a value of three or more labels or one whose first label is universe, is
            neither, and is refused (request.validation).
        x_request_id (str | Unset):
        x_correlation_id (str | Unset):
        traceparent (str | Unset):
        body (ExportRequest):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        ErrorEnvelope | File
    """

    return (
        await asyncio_detailed(
            workspace=workspace,
            client=client,
            body=body,
            x_request_id=x_request_id,
            x_correlation_id=x_correlation_id,
            traceparent=traceparent,
        )
    ).parsed
