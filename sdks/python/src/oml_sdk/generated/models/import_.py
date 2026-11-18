from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, Self, TypeVar, cast

from attrs import define as _attrs_define

from ..models.import_state import ImportState

if TYPE_CHECKING:
    from ..models.import_counts import ImportCounts
    from ..models.import_refusals_item import ImportRefusalsItem
    from ..models.import_source import ImportSource


T = TypeVar("T", bound="Import")


@_attrs_define
class Import:
    """One import as its importing key reads it (ADR-0084 d11, d15).

    Attributes:
        import_id (str):
        state (ImportState):
        target_workspace_id (str):
        upload_id (str):
        source (ImportSource):
        manifest_digest (str):
        signer_fingerprint (str):
        authenticated (bool):
        allow_older (bool):
        position (int): The versions the import recorded.
        counts (ImportCounts): By outcome (new, changed, removed, unchanged, equivalent, conflict, refused), and
            edges_proposed, edges_reported and erasure_markers.
        refusals (list[ImportRefusalsItem]):
        created_at (str):
        ended_at (None | str):
    """

    import_id: str
    state: ImportState
    target_workspace_id: str
    upload_id: str
    source: ImportSource
    manifest_digest: str
    signer_fingerprint: str
    authenticated: bool
    allow_older: bool
    position: int
    counts: ImportCounts
    refusals: list[ImportRefusalsItem]
    created_at: str
    ended_at: None | str

    def to_dict(self) -> dict[str, Any]:
        import_id = self.import_id

        state = self.state.value

        target_workspace_id = self.target_workspace_id

        upload_id = self.upload_id

        source = self.source.to_dict()

        manifest_digest = self.manifest_digest

        signer_fingerprint = self.signer_fingerprint

        authenticated = self.authenticated

        allow_older = self.allow_older

        position = self.position

        counts = self.counts.to_dict()

        refusals = []
        for refusals_item_data in self.refusals:
            refusals_item = refusals_item_data.to_dict()
            refusals.append(refusals_item)

        created_at = self.created_at

        ended_at: None | str
        ended_at = self.ended_at

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "import_id": import_id,
                "state": state,
                "target_workspace_id": target_workspace_id,
                "upload_id": upload_id,
                "source": source,
                "manifest_digest": manifest_digest,
                "signer_fingerprint": signer_fingerprint,
                "authenticated": authenticated,
                "allow_older": allow_older,
                "position": position,
                "counts": counts,
                "refusals": refusals,
                "created_at": created_at,
                "ended_at": ended_at,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.import_counts import ImportCounts
        from ..models.import_refusals_item import ImportRefusalsItem
        from ..models.import_source import ImportSource

        d = dict(src_dict)
        import_id = d.pop("import_id")

        state = ImportState(d.pop("state"))

        target_workspace_id = d.pop("target_workspace_id")

        upload_id = d.pop("upload_id")

        source = ImportSource.from_dict(d.pop("source"))

        manifest_digest = d.pop("manifest_digest")

        signer_fingerprint = d.pop("signer_fingerprint")

        authenticated = d.pop("authenticated")

        allow_older = d.pop("allow_older")

        position = d.pop("position")

        counts = ImportCounts.from_dict(d.pop("counts"))

        refusals = []
        _refusals = d.pop("refusals")
        for refusals_item_data in _refusals:
            refusals_item = ImportRefusalsItem.from_dict(refusals_item_data)

            refusals.append(refusals_item)

        created_at = d.pop("created_at")

        def _parse_ended_at(data: object) -> None | str:
            if data is None:
                return data
            return cast(None | str, data)

        ended_at = _parse_ended_at(d.pop("ended_at"))

        import_ = cls(
            import_id=import_id,
            state=state,
            target_workspace_id=target_workspace_id,
            upload_id=upload_id,
            source=source,
            manifest_digest=manifest_digest,
            signer_fingerprint=signer_fingerprint,
            authenticated=authenticated,
            allow_older=allow_older,
            position=position,
            counts=counts,
            refusals=refusals,
            created_at=created_at,
            ended_at=ended_at,
        )

        return import_
