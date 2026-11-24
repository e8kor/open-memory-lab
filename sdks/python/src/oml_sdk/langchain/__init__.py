"""LangChain and LangGraph over `OmlClient` (ADR-0094).

`OmlRetriever` and the two tools of `oml_tools` read and write in any
workspace the key reaches; `OmlStore`, a LangGraph `BaseStore`, works only in
an in-progress workspace (ADR-0030) and refuses any other with `StoreRefused`.

This module needs the `langchain` extra (`pip install
'open-memory-lab-sdk[langchain]'`); `import oml_sdk` never imports it.
"""

from __future__ import annotations

try:
    import langchain_core  # noqa: F401
    import langgraph.store.base  # noqa: F401
except ImportError as missing:  # pragma: no cover - exercised only without the extra
    raise ImportError(
        "oml_sdk.langchain needs the langchain extra: pip install 'open-memory-lab-sdk[langchain]'"
    ) from missing

from .retriever import OmlRetriever, document_for
from .store import STORE_PREFIX, OmlStore, StoreRefused, claim_key_for
from .tools import oml_tools, recall_tool, remember_tool

__all__ = [
    "STORE_PREFIX",
    "OmlRetriever",
    "OmlStore",
    "StoreRefused",
    "claim_key_for",
    "document_for",
    "oml_tools",
    "recall_tool",
    "remember_tool",
]
