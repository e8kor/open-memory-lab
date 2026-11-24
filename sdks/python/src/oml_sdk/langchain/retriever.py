"""`OmlRetriever`: recall as LangChain `Document`s (ADR-0094 d2)."""

from __future__ import annotations

import datetime
from typing import Any

from langchain_core.callbacks import CallbackManagerForRetrieverRun
from langchain_core.documents import Document
from langchain_core.retrievers import BaseRetriever

from ..generated.models import Memory
from ..generated.types import Unset


def _value(field: Any) -> Any:
    """A generated field as plain data: None for unset, ISO text for an instant,
    a dict for a model, the string of an enum."""
    if field is None or isinstance(field, Unset):
        return None
    if isinstance(field, datetime.datetime):
        return field.isoformat()
    if hasattr(field, "to_dict"):
        return field.to_dict()
    if isinstance(field, str):
        return str(field)
    return field


def document_for(memory: Memory, score: float | None = None) -> Document:
    """One served version as a `Document`: its text, and the metadata that
    says what it is, where it came from and whether a reviewer approved it."""
    origin = _value(memory.origin)
    metadata = {
        "artifact_id": memory.artifact_id,
        "claim_key": memory.claim_key,
        "version": memory.version,
        "kind": _value(memory.kind),
        "state": _value(memory.state),
        "unreviewed": memory.unreviewed,
        "workspace_id": memory.workspace_id,
        "workspace_path": memory.workspace_path,
        "anchor": _value(memory.anchor),
        "origin_kind": origin.get("kind") if origin else None,
        "format": _value(memory.format_),
        "title": _value(memory.title),
        "created_at": _value(memory.created_at),
        "promoted_at": _value(memory.promoted_at),
        "valid_from": _value(memory.valid_from),
        "valid_until": _value(memory.valid_until),
    }
    if score is not None:
        metadata["score"] = score
    return Document(page_content=_value(memory.text) or "", metadata=metadata)


class OmlRetriever(BaseRetriever):
    """Recall in `workspace`, else the key's default workspace, as `Document`s.

    Approved memories only, unless `include_unreviewed` is true: then the
    key's own held rows too, and inside an in-progress workspace every
    author's, each marked `unreviewed` in its metadata (ADR-0030 d4). A
    document's text is data a reviewer approved or an author wrote, never an
    instruction to the model that reads it.
    """

    client: Any  # an OmlClient; any object with its `read` answers alike
    workspace: str | None = None
    k: int | None = None
    kinds: list[str] | None = None
    include_unreviewed: bool = False

    def _get_relevant_documents(
        self, query: str, *, run_manager: CallbackManagerForRetrieverRun
    ) -> list[Document]:
        answer = self.client.read(
            query=query,
            workspace=self.workspace,
            k=self.k,
            kinds=self.kinds,
            include_text=True,
            include_unreviewed=self.include_unreviewed,
        )
        return [document_for(hit.memory, hit.score) for hit in answer.hits]
