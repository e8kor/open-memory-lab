from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, Self, TypeVar, cast

from attrs import define as _attrs_define

if TYPE_CHECKING:
    from ..models.import_plan_counts import ImportPlanCounts
    from ..models.import_plan_edges import ImportPlanEdges
    from ..models.import_plan_keys_item import ImportPlanKeysItem
    from ..models.import_plan_source import ImportPlanSource


T = TypeVar("T", bound="ImportPlan")


@_attrs_define
class ImportPlan:
    """What an import would write (ADR-0084 d8, R10).

    Attributes:
        source (ImportPlanSource):
        exported_at (str):
        manifest_digest (str):
        target_workspace_id (None | str): Null for a new name the import would create.
        authenticated (bool): The signature was checked against expect_key; otherwise it proves integrity only (d5).
        signer_fingerprint (str):
        allow_older (bool):
        counts (ImportPlanCounts):
        keys (list[ImportPlanKeysItem]):
        erasure_markers (int): Markers for versions already imported (d13).
        edges (ImportPlanEdges):
    """

    source: ImportPlanSource
    exported_at: str
    manifest_digest: str
    target_workspace_id: None | str
    authenticated: bool
    signer_fingerprint: str
    allow_older: bool
    counts: ImportPlanCounts
    keys: list[ImportPlanKeysItem]
    erasure_markers: int
    edges: ImportPlanEdges

    def to_dict(self) -> dict[str, Any]:
        source = self.source.to_dict()

        exported_at = self.exported_at

        manifest_digest = self.manifest_digest

        target_workspace_id: None | str
        target_workspace_id = self.target_workspace_id

        authenticated = self.authenticated

        signer_fingerprint = self.signer_fingerprint

        allow_older = self.allow_older

        counts = self.counts.to_dict()

        keys = []
        for keys_item_data in self.keys:
            keys_item = keys_item_data.to_dict()
            keys.append(keys_item)

        erasure_markers = self.erasure_markers

        edges = self.edges.to_dict()

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "source": source,
                "exported_at": exported_at,
                "manifest_digest": manifest_digest,
                "target_workspace_id": target_workspace_id,
                "authenticated": authenticated,
                "signer_fingerprint": signer_fingerprint,
                "allow_older": allow_older,
                "counts": counts,
                "keys": keys,
                "erasure_markers": erasure_markers,
                "edges": edges,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.import_plan_counts import ImportPlanCounts
        from ..models.import_plan_edges import ImportPlanEdges
        from ..models.import_plan_keys_item import ImportPlanKeysItem
        from ..models.import_plan_source import ImportPlanSource

        d = dict(src_dict)
        source = ImportPlanSource.from_dict(d.pop("source"))

        exported_at = d.pop("exported_at")

        manifest_digest = d.pop("manifest_digest")

        def _parse_target_workspace_id(data: object) -> None | str:
            if data is None:
                return data
            return cast(None | str, data)

        target_workspace_id = _parse_target_workspace_id(d.pop("target_workspace_id"))

        authenticated = d.pop("authenticated")

        signer_fingerprint = d.pop("signer_fingerprint")

        allow_older = d.pop("allow_older")

        counts = ImportPlanCounts.from_dict(d.pop("counts"))

        keys = []
        _keys = d.pop("keys")
        for keys_item_data in _keys:
            keys_item = ImportPlanKeysItem.from_dict(keys_item_data)

            keys.append(keys_item)

        erasure_markers = d.pop("erasure_markers")

        edges = ImportPlanEdges.from_dict(d.pop("edges"))

        import_plan = cls(
            source=source,
            exported_at=exported_at,
            manifest_digest=manifest_digest,
            target_workspace_id=target_workspace_id,
            authenticated=authenticated,
            signer_fingerprint=signer_fingerprint,
            allow_older=allow_older,
            counts=counts,
            keys=keys,
            erasure_markers=erasure_markers,
            edges=edges,
        )

        return import_plan
