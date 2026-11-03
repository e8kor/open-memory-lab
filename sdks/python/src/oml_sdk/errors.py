"""One exception class per error family of docs/api/errors.md.

docs/engineering/sdks.md, "What the wrapper covers": the class is chosen by the
code's family prefix, so a code this client has never seen lands in its
family's class and a family it has never seen lands in `ApiError`. Error bodies
are read here, not through the generated `ErrorEnvelope`, whose `ErrorCode`
enum refuses any code the spec it was generated from does not list.
"""

from __future__ import annotations

import json
from collections.abc import Mapping
from typing import Any


class OmlError(Exception):
    """Every error this package raises."""


class ConfigError(OmlError):
    """The client has no key or no server URL; nothing was sent."""


class NotBuiltError(OmlError):
    """The server does not serve this capability yet; nothing was sent."""


class TransportError(OmlError):
    """The request failed in transit after the last retry; the cause is chained."""


class ApiError(OmlError):
    """A refusal in the errors.md envelope. `code` is None only when the
    response carried no envelope (a proxy's error page, say)."""

    def __init__(
        self,
        *,
        status: int,
        code: str | None,
        message: str,
        rule: str | None,
        retryable: bool,
        details: Mapping[str, Any] | None,
        request_id: str | None,
        retry_after: float | None,
    ) -> None:
        super().__init__(message)
        self.status = status
        self.code = code
        self.message = message
        self.rule = rule
        self.retryable = retryable
        self.details = dict(details) if details else {}
        self.request_id = request_id
        self.retry_after = retry_after

    def __str__(self) -> str:
        text = f"{self.code or f'HTTP {self.status}'}: {self.message}"
        if self.rule:
            text += f" (rule: {self.rule})"
        if self.request_id:
            text += f" [{self.request_id}]"
        return text

    def __repr__(self) -> str:
        return f"{type(self).__name__}(status={self.status}, code={self.code!r})"


class AuthError(ApiError):
    """`auth`: who is calling."""


class AccessError(ApiError):
    """`access`: what this key may do."""


class RequestError(ApiError):
    """`request`: the request itself."""


class IdempotencyError(ApiError):
    """`idempotency`: the replay contract."""


class SubmissionError(ApiError):
    """`submission`: admission refusals."""


class NamingError(ApiError):
    """`name`: the unique names of workareas, crews, keys and tenants (named so
    beside Python's NameError)."""


class WorkareaError(ApiError):
    """`workarea`: structure and status refusals."""


class ReviewError(ApiError):
    """`review`: independence and the serve binding."""


class SimulationError(ApiError):
    """`simulation`: the fence."""


class VocabError(ApiError):
    """`vocab`: vocabulary structure refusals."""


class UploadError(ApiError):
    """`upload`: attachment sessions."""


class LifecycleError(ApiError):
    """`lifecycle`: erased content."""


class LimitError(ApiError):
    """`limit`: rate and quota."""


class ExportError(ApiError):
    """`export`: evidence export jobs."""


class SettingsError(ApiError):
    """`settings`: the settings document."""


class ServerError(ApiError):
    """`system`: the replica cannot serve (named so beside Python's SystemError)."""


FAMILIES: dict[str, type[ApiError]] = {
    "auth": AuthError,
    "access": AccessError,
    "request": RequestError,
    "idempotency": IdempotencyError,
    "submission": SubmissionError,
    "name": NamingError,
    "workarea": WorkareaError,
    "review": ReviewError,
    "simulation": SimulationError,
    "vocab": VocabError,
    "upload": UploadError,
    "lifecycle": LifecycleError,
    "limit": LimitError,
    "export": ExportError,
    "settings": SettingsError,
    "system": ServerError,
}


def _retry_after(headers: Mapping[str, str]) -> float | None:
    value = next((v for k, v in headers.items() if k.lower() == "retry-after"), None)
    try:
        return max(0.0, float(value)) if value is not None else None
    except ValueError:
        return None


def error_from_response(status: int, content: bytes, headers: Mapping[str, str]) -> ApiError:
    """The typed exception for one non-2xx response."""
    try:
        body = json.loads(content)
        error = body["error"]
        code = error["code"]
        if not isinstance(code, str):
            raise TypeError(code)
    except (ValueError, TypeError, KeyError):
        return ApiError(
            status=status,
            code=None,
            message=f"HTTP {status} without the error envelope",
            rule=None,
            retryable=False,
            details=None,
            request_id=None,
            retry_after=_retry_after(headers),
        )
    cls = FAMILIES.get(code.split(".", 1)[0], ApiError)
    details = error.get("details")
    request_id = body.get("request_id")
    return cls(
        status=status,
        code=code,
        message=str(error.get("message", "")),
        rule=error.get("rule") if isinstance(error.get("rule"), str) else None,
        retryable=error.get("retryable") is True,
        details=details if isinstance(details, Mapping) else None,
        request_id=request_id if isinstance(request_id, str) else None,
        retry_after=_retry_after(headers),
    )
