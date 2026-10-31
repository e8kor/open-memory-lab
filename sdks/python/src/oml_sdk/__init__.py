"""oml_sdk — a Python client for the Open Memory Lab HTTP API.

`oml_sdk.generated` is generated from docs/api/openapi.yaml and never edited;
`OmlClient` is the hand-written wrapper over it (docs/engineering/sdks.md).
"""

from .client import OmlClient, Written
from .errors import (
    AccessError,
    ApiError,
    AuthError,
    ConfigError,
    ExportError,
    IdempotencyError,
    LifecycleError,
    LimitError,
    NamingError,
    NotBuiltError,
    OmlError,
    RequestError,
    ReviewError,
    ServerError,
    SettingsError,
    SimulationError,
    SubmissionError,
    TransportError,
    UploadError,
    VocabError,
    WorkareaError,
)

__all__ = [
    "AccessError",
    "ApiError",
    "AuthError",
    "ConfigError",
    "ExportError",
    "IdempotencyError",
    "LifecycleError",
    "LimitError",
    "NamingError",
    "NotBuiltError",
    "OmlClient",
    "OmlError",
    "RequestError",
    "ReviewError",
    "ServerError",
    "SettingsError",
    "SimulationError",
    "SubmissionError",
    "TransportError",
    "UploadError",
    "VocabError",
    "WorkareaError",
    "Written",
]
