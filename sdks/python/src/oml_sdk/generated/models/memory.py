from __future__ import annotations

import datetime
from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, Self, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.memory_summary_kind import MemorySummaryKind
from ..models.memory_summary_provenance import MemorySummaryProvenance
from ..models.memory_summary_severity import MemorySummarySeverity
from ..models.memory_summary_state import MemorySummaryState
from ..models.stale import Stale
from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.anchor import Anchor
    from ..models.approver import Approver
    from ..models.attachment_descriptor import AttachmentDescriptor
    from ..models.document_fields import DocumentFields
    from ..models.memory_summary_replaces_type_0 import MemorySummaryReplacesType0
    from ..models.origin import Origin
    from ..models.scan_finding import ScanFinding
    from ..models.skill_fields import SkillFields
    from ..models.source_checked import SourceChecked
    from ..models.test_fields import TestFields


T = TypeVar("T", bound="Memory")


@_attrs_define
class Memory:
    """
    Attributes:
        artifact_id (str):
        claim_key (str):
        version (int):
        tip (bool): Whether this version is the served tip of its claim for the calling key, audience-scoped: for a key
            in a simulation's audience an overlay tip that shadows the default tip, which then reads tip false; for every
            other key the default tip (docs/architecture/audit-and-lineage.md). A superseded version is state promoted, tip
            false.
        workspace_id (None | str): Null when the calling key holds no role on the node (naming a node,
            docs/architecture/roles-and-keys.md, Own) — possible only on an own row an admin moved there (ADR-0025 d3).
        workspace_path (None | str): Derived at read time; grants bind to node ids. Null wherever workspace_id is.
        kind (MemorySummaryKind):
        state (MemorySummaryState): withdrawn is a held version its author retracted, kept and never served (ADR-0083
            d13; since v0.8.3).
        unreviewed (bool): True exactly when the served version is held: the caller's own held version, or, inside a
            workspace a person put in progress, another author's held memory served to a reader there (ADR-0030 d3, d4).
        tokens (int): Budget tokens of the body (counter oml-tc/1): ceil(UTF-8 bytes of the NFC body / 4); not any
            model's count (docs/architecture/retrieval.md, Token budgets).
        retirement (bool): True on a retirement version; a claim whose served tip is one is a retired claim, which
            recall does not serve. Default: False.
        replaces (MemorySummaryReplacesType0 | None): For a supersession, the version it replaces; null for a version 1.
            It obeys the same readability test as derives_from on every read, so it is also null when the caller cannot read
            the replaced version — after an admin moved that version into another workspace or back into held, say (ADR-0025
            d3).
        held_expires_at (datetime.datetime | None): For a held row, when held-item expiry dead-letters it under the
            workspace's review policy; null on every other row, and on every row while held-item expiry is unbuilt (ADR-0025
            d13).
        created_at (datetime.datetime):
        promoted_at (datetime.datetime | None):
        anchor (Anchor | None): The version's anchor, as its author declared it, or null (ADR-0057 d1, d2).
        stale (None | Stale): changed or removed from the served version's latest source check; null otherwise, and
            always null on a held version (ADR-0057 d5).
        source_checked (None | SourceChecked): The version's latest source check, or null; always null on a held version
            (ADR-0057 d5).
        severity (MemorySummarySeverity | Unset): On kind guardrail alone, except a retirement version (ADR-0037 d2).
        rule (Any | Unset): On kind guardrail alone: its oml-gr/1 rule, or null when it has none (ADR-0037 d8).
        skill (SkillFields | Unset): A skill's name and description, signed with the version (ADR-0080 d1).
        document (DocumentFields | Unset): A document's title, signed with the version (ADR-0081 d1).
        test (TestFields | Unset): A test case's shape (ADR-0086 d1).
        load_warning (str | Unset): On a held guardrail alone: every reader of it is told that every key reading its
            workspace will load the rule once it binds (ADR-0037 d10).
        scan_findings (list[ScanFinding] | Unset): Every finding stored on the version, in the order recorded, on the
            review serve, the held listing and the named read alone; absent elsewhere, so an empty list means the scan found
            nothing (ADR-0074 d17, d23).
        principal (None | str | Unset): The authoring principal; null on a version in universe served to a key outside
            its authoring key set, which is served with derives_from empty and workspace_path universe, so no field names
            its source (ADR-0044 d9).
        author_key_id (None | str | Unset): The authoring key's id; null wherever principal is (ADR-0044 d9). The web
            console names the kind of its person's own keys by it and narrows a search to one of them (ADR-0070 d5). Since
            v0.7.0.
        scan_finding (None | str | Unset): Admission's scan finding on the version, class: rule at place, never the
            matched text; null when the scan found nothing, and null wherever principal is (ADR-0065 d8; ADR-0044 d9;
            ADR-0070 d5). Since v0.7.0.
        provenance (MemorySummaryProvenance | Unset): admin_fix for a version an admin fix wrote (an edit or a
            retirement), whose principal and author_key_id are then null: no reader learns which admin (ADR-0089 d7). Since
            v0.8.4 for admin_fix.
        simulation_id (None | str | Unset):
        self_signed (bool | Unset): Marked rows under the bounded development exception; never promote beyond their
            workspace.
        plaintext_digest (str | Unset): SHA-256 of plaintext unless a field says ciphertext.
        derives_from (list[str] | Unset): On get, recall and every listing, only the ancestors the caller can read — its
            own rows in any state, promoted rows within its reader effective access (ADR-0025 d3); the lineage read shows
            the full closure, elided where unreadable.
        attachments (list[AttachmentDescriptor] | Unset):
        approved_at (datetime.datetime | None | Unset): When the approving verdict that ratified the version was given:
            the verdict's own time, at or just before promoted_at. Null on a version that is not promoted, wherever
            principal is null (ADR-0044 d9), and on a version ratified before the journal named its verdict (ADR-0077 d1,
            d3, d4). Since v0.7.2.
        approver (Approver | None | Unset): What kind of reviewer gave the approving verdict that ratified the version,
            never who; null wherever approved_at is (ADR-0077 d2, d4). Since v0.7.2.
        origin (None | Origin | Unset): What kind of source the version's text came from, as signed; null on versions
            written before ADR-0063 (d1-d3).
        valid_from (None | str | Unset): When the version says it begins to hold, as signed: a period (YYYY, YYYY-MM,
            YYYY-MM-DD) or an instant; null when it declares none (ADR-0059 d2).
        valid_until (None | str | Unset): When it stops holding, as signed, or the instant a ratified supersession ended
            it at; null when open (ADR-0059 d2, d4).
        format_ (str | Unset): The media type signed with the version; text/plain for every version written before
            formats (ADR-0056 d1, d12).
        title (None | str | Unset): The title signed with the version, or null (ADR-0056 d10).
        search_text (str | Unset): For a format whose text is not prose, what the claim will be found by, derived from
            its source; absent for prose, so a reviewer sees it on the card before the verdict (ADR-0056 d9; ADR-0070 d5).
            Since v0.6.4.
        replaces_valid_until (str | Unset): On a held successor alone: the instant approving it ends the version it
            replaces at, the successor's declared valid_from, so a reviewer sees it before the verdict; absent when
            approving ends nothing (ADR-0059 d4).
        text (str | Unset): The claim's text, decrypted on the serving path only; v0.3.0 named it body (ADR-0039 d7).
            Absent from a recall item sent with include_text false, and from a listing item unless include_text is true
            (ADR-0070 d5). A skill hit's text is its description, never its body (ADR-0092 d3).
        needs_person (bool | Unset): On a catalog entry, and on a held instruction hit: only a person's verdict promotes
            the version (ADR-0080 d16; ADR-0092 d3, d6).
        name (str | Unset): A skill's name, on a skill hit and a skill's catalog entry (ADR-0092 d3, d6).
        description (str | Unset): A skill's description; on a memory or derived fact's catalog entry, a cut of its text
            of at most 160 characters, marked preview (ADR-0092 d6, d11 answer 7).
        preview (bool | Unset): true when description is the server's cut of the text (ADR-0092 d6).
        uri (str | Unset): A skill's or a document's resource, oml://workspace/<workspace>/skill/<name> or …/doc/<path>
            (ADR-0080 d7; ADR-0081 d6; ADR-0092 d3, d6).
        prompt (None | str | Unset): The command prompts/list offers a skill under at the read's scope node; null on a
            catalog entry the scope does not offer (ADR-0092 d3, d6).
        in_force (bool | Unset): A guardrail: whether it binds at the read's scope node; true on every guardrail hit,
            since only an applicable one is a hit (ADR-0092 d3, d6).
        applies_here (bool | Unset): A steering catalog entry: whether it is in the steering list at the read's scope
            node (ADR-0092 d6).
        size (int | Unset): A document catalog entry's body bytes (ADR-0092 d6).
    """

    artifact_id: str
    claim_key: str
    version: int
    tip: bool
    workspace_id: None | str
    workspace_path: None | str
    kind: MemorySummaryKind
    state: MemorySummaryState
    unreviewed: bool
    tokens: int
    replaces: MemorySummaryReplacesType0 | None
    held_expires_at: datetime.datetime | None
    created_at: datetime.datetime
    promoted_at: datetime.datetime | None
    anchor: Anchor | None
    stale: None | Stale
    source_checked: None | SourceChecked
    retirement: bool = False
    severity: MemorySummarySeverity | Unset = UNSET
    rule: Any | Unset = UNSET
    skill: SkillFields | Unset = UNSET
    document: DocumentFields | Unset = UNSET
    test: TestFields | Unset = UNSET
    load_warning: str | Unset = UNSET
    scan_findings: list[ScanFinding] | Unset = UNSET
    principal: None | str | Unset = UNSET
    author_key_id: None | str | Unset = UNSET
    scan_finding: None | str | Unset = UNSET
    provenance: MemorySummaryProvenance | Unset = UNSET
    simulation_id: None | str | Unset = UNSET
    self_signed: bool | Unset = UNSET
    plaintext_digest: str | Unset = UNSET
    derives_from: list[str] | Unset = UNSET
    attachments: list[AttachmentDescriptor] | Unset = UNSET
    approved_at: datetime.datetime | None | Unset = UNSET
    approver: Approver | None | Unset = UNSET
    origin: None | Origin | Unset = UNSET
    valid_from: None | str | Unset = UNSET
    valid_until: None | str | Unset = UNSET
    format_: str | Unset = UNSET
    title: None | str | Unset = UNSET
    search_text: str | Unset = UNSET
    replaces_valid_until: str | Unset = UNSET
    text: str | Unset = UNSET
    needs_person: bool | Unset = UNSET
    name: str | Unset = UNSET
    description: str | Unset = UNSET
    preview: bool | Unset = UNSET
    uri: str | Unset = UNSET
    prompt: None | str | Unset = UNSET
    in_force: bool | Unset = UNSET
    applies_here: bool | Unset = UNSET
    size: int | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        from ..models.anchor import Anchor
        from ..models.approver import Approver
        from ..models.memory_summary_replaces_type_0 import MemorySummaryReplacesType0
        from ..models.origin import Origin
        from ..models.source_checked import SourceChecked

        artifact_id = self.artifact_id

        claim_key = self.claim_key

        version = self.version

        tip = self.tip

        workspace_id: None | str
        workspace_id = self.workspace_id

        workspace_path: None | str
        workspace_path = self.workspace_path

        kind = self.kind.value

        state = self.state.value

        unreviewed = self.unreviewed

        tokens = self.tokens

        retirement = self.retirement

        replaces: dict[str, Any] | None
        if isinstance(self.replaces, MemorySummaryReplacesType0):
            replaces = self.replaces.to_dict()
        else:
            replaces = self.replaces

        held_expires_at: None | str
        if isinstance(self.held_expires_at, datetime.datetime):
            held_expires_at = self.held_expires_at.isoformat()
        else:
            held_expires_at = self.held_expires_at

        created_at = self.created_at.isoformat()

        promoted_at: None | str
        if isinstance(self.promoted_at, datetime.datetime):
            promoted_at = self.promoted_at.isoformat()
        else:
            promoted_at = self.promoted_at

        anchor: dict[str, Any] | None
        if isinstance(self.anchor, Anchor):
            anchor = self.anchor.to_dict()
        else:
            anchor = self.anchor

        stale: None | str
        if isinstance(self.stale, Stale):
            stale = self.stale.value
        else:
            stale = self.stale

        source_checked: dict[str, Any] | None
        if isinstance(self.source_checked, SourceChecked):
            source_checked = self.source_checked.to_dict()
        else:
            source_checked = self.source_checked

        severity: str | Unset = UNSET
        if not isinstance(self.severity, Unset):
            severity = self.severity.value

        rule = self.rule

        skill: dict[str, Any] | Unset = UNSET
        if not isinstance(self.skill, Unset):
            skill = self.skill.to_dict()

        document: dict[str, Any] | Unset = UNSET
        if not isinstance(self.document, Unset):
            document = self.document.to_dict()

        test: dict[str, Any] | Unset = UNSET
        if not isinstance(self.test, Unset):
            test = self.test.to_dict()

        load_warning = self.load_warning

        scan_findings: list[dict[str, Any]] | Unset = UNSET
        if not isinstance(self.scan_findings, Unset):
            scan_findings = []
            for scan_findings_item_data in self.scan_findings:
                scan_findings_item = scan_findings_item_data.to_dict()
                scan_findings.append(scan_findings_item)

        principal: None | str | Unset
        if isinstance(self.principal, Unset):
            principal = UNSET
        else:
            principal = self.principal

        author_key_id: None | str | Unset
        if isinstance(self.author_key_id, Unset):
            author_key_id = UNSET
        else:
            author_key_id = self.author_key_id

        scan_finding: None | str | Unset
        if isinstance(self.scan_finding, Unset):
            scan_finding = UNSET
        else:
            scan_finding = self.scan_finding

        provenance: str | Unset = UNSET
        if not isinstance(self.provenance, Unset):
            provenance = self.provenance.value

        simulation_id: None | str | Unset
        if isinstance(self.simulation_id, Unset):
            simulation_id = UNSET
        else:
            simulation_id = self.simulation_id

        self_signed = self.self_signed

        plaintext_digest = self.plaintext_digest

        derives_from: list[str] | Unset = UNSET
        if not isinstance(self.derives_from, Unset):
            derives_from = self.derives_from

        attachments: list[dict[str, Any]] | Unset = UNSET
        if not isinstance(self.attachments, Unset):
            attachments = []
            for attachments_item_data in self.attachments:
                attachments_item = attachments_item_data.to_dict()
                attachments.append(attachments_item)

        approved_at: None | str | Unset
        if isinstance(self.approved_at, Unset):
            approved_at = UNSET
        elif isinstance(self.approved_at, datetime.datetime):
            approved_at = self.approved_at.isoformat()
        else:
            approved_at = self.approved_at

        approver: dict[str, Any] | None | Unset
        if isinstance(self.approver, Unset):
            approver = UNSET
        elif isinstance(self.approver, Approver):
            approver = self.approver.to_dict()
        else:
            approver = self.approver

        origin: dict[str, Any] | None | Unset
        if isinstance(self.origin, Unset):
            origin = UNSET
        elif isinstance(self.origin, Origin):
            origin = self.origin.to_dict()
        else:
            origin = self.origin

        valid_from: None | str | Unset
        if isinstance(self.valid_from, Unset):
            valid_from = UNSET
        else:
            valid_from = self.valid_from

        valid_until: None | str | Unset
        if isinstance(self.valid_until, Unset):
            valid_until = UNSET
        else:
            valid_until = self.valid_until

        format_ = self.format_

        title: None | str | Unset
        if isinstance(self.title, Unset):
            title = UNSET
        else:
            title = self.title

        search_text = self.search_text

        replaces_valid_until = self.replaces_valid_until

        text = self.text

        needs_person = self.needs_person

        name = self.name

        description = self.description

        preview = self.preview

        uri = self.uri

        prompt: None | str | Unset
        if isinstance(self.prompt, Unset):
            prompt = UNSET
        else:
            prompt = self.prompt

        in_force = self.in_force

        applies_here = self.applies_here

        size = self.size

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "artifact_id": artifact_id,
                "claim_key": claim_key,
                "version": version,
                "tip": tip,
                "workspace_id": workspace_id,
                "workspace_path": workspace_path,
                "kind": kind,
                "state": state,
                "unreviewed": unreviewed,
                "tokens": tokens,
                "retirement": retirement,
                "replaces": replaces,
                "held_expires_at": held_expires_at,
                "created_at": created_at,
                "promoted_at": promoted_at,
                "anchor": anchor,
                "stale": stale,
                "source_checked": source_checked,
            }
        )
        if severity is not UNSET:
            field_dict["severity"] = severity
        if rule is not UNSET:
            field_dict["rule"] = rule
        if skill is not UNSET:
            field_dict["skill"] = skill
        if document is not UNSET:
            field_dict["document"] = document
        if test is not UNSET:
            field_dict["test"] = test
        if load_warning is not UNSET:
            field_dict["load_warning"] = load_warning
        if scan_findings is not UNSET:
            field_dict["scan_findings"] = scan_findings
        if principal is not UNSET:
            field_dict["principal"] = principal
        if author_key_id is not UNSET:
            field_dict["author_key_id"] = author_key_id
        if scan_finding is not UNSET:
            field_dict["scan_finding"] = scan_finding
        if provenance is not UNSET:
            field_dict["provenance"] = provenance
        if simulation_id is not UNSET:
            field_dict["simulation_id"] = simulation_id
        if self_signed is not UNSET:
            field_dict["self_signed"] = self_signed
        if plaintext_digest is not UNSET:
            field_dict["plaintext_digest"] = plaintext_digest
        if derives_from is not UNSET:
            field_dict["derives_from"] = derives_from
        if attachments is not UNSET:
            field_dict["attachments"] = attachments
        if approved_at is not UNSET:
            field_dict["approved_at"] = approved_at
        if approver is not UNSET:
            field_dict["approver"] = approver
        if origin is not UNSET:
            field_dict["origin"] = origin
        if valid_from is not UNSET:
            field_dict["valid_from"] = valid_from
        if valid_until is not UNSET:
            field_dict["valid_until"] = valid_until
        if format_ is not UNSET:
            field_dict["format"] = format_
        if title is not UNSET:
            field_dict["title"] = title
        if search_text is not UNSET:
            field_dict["search_text"] = search_text
        if replaces_valid_until is not UNSET:
            field_dict["replaces_valid_until"] = replaces_valid_until
        if text is not UNSET:
            field_dict["text"] = text
        if needs_person is not UNSET:
            field_dict["needs_person"] = needs_person
        if name is not UNSET:
            field_dict["name"] = name
        if description is not UNSET:
            field_dict["description"] = description
        if preview is not UNSET:
            field_dict["preview"] = preview
        if uri is not UNSET:
            field_dict["uri"] = uri
        if prompt is not UNSET:
            field_dict["prompt"] = prompt
        if in_force is not UNSET:
            field_dict["in_force"] = in_force
        if applies_here is not UNSET:
            field_dict["applies_here"] = applies_here
        if size is not UNSET:
            field_dict["size"] = size

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.anchor import Anchor
        from ..models.approver import Approver
        from ..models.attachment_descriptor import AttachmentDescriptor
        from ..models.document_fields import DocumentFields
        from ..models.memory_summary_replaces_type_0 import MemorySummaryReplacesType0
        from ..models.origin import Origin
        from ..models.scan_finding import ScanFinding
        from ..models.skill_fields import SkillFields
        from ..models.source_checked import SourceChecked
        from ..models.test_fields import TestFields

        d = dict(src_dict)
        artifact_id = d.pop("artifact_id")

        claim_key = d.pop("claim_key")

        version = d.pop("version")

        tip = d.pop("tip")

        def _parse_workspace_id(data: object) -> None | str:
            if data is None:
                return data
            return cast(None | str, data)

        workspace_id = _parse_workspace_id(d.pop("workspace_id"))

        def _parse_workspace_path(data: object) -> None | str:
            if data is None:
                return data
            return cast(None | str, data)

        workspace_path = _parse_workspace_path(d.pop("workspace_path"))

        kind = MemorySummaryKind(d.pop("kind"))

        state = MemorySummaryState(d.pop("state"))

        unreviewed = d.pop("unreviewed")

        tokens = d.pop("tokens")

        retirement = d.pop("retirement")

        def _parse_replaces(data: object) -> MemorySummaryReplacesType0 | None:
            if data is None:
                return data
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                replaces_type_0 = MemorySummaryReplacesType0.from_dict(data)

                return replaces_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(MemorySummaryReplacesType0 | None, data)

        replaces = _parse_replaces(d.pop("replaces"))

        def _parse_held_expires_at(data: object) -> datetime.datetime | None:
            if data is None:
                return data
            try:
                if not isinstance(data, str):
                    raise TypeError()
                held_expires_at_type_0 = datetime.datetime.fromisoformat(data)

                return held_expires_at_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(datetime.datetime | None, data)

        held_expires_at = _parse_held_expires_at(d.pop("held_expires_at"))

        created_at = datetime.datetime.fromisoformat(d.pop("created_at"))

        def _parse_promoted_at(data: object) -> datetime.datetime | None:
            if data is None:
                return data
            try:
                if not isinstance(data, str):
                    raise TypeError()
                promoted_at_type_0 = datetime.datetime.fromisoformat(data)

                return promoted_at_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(datetime.datetime | None, data)

        promoted_at = _parse_promoted_at(d.pop("promoted_at"))

        def _parse_anchor(data: object) -> Anchor | None:
            if data is None:
                return data
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                anchor_type_0 = Anchor.from_dict(data)

                return anchor_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(Anchor | None, data)

        anchor = _parse_anchor(d.pop("anchor"))

        def _parse_stale(data: object) -> None | Stale:
            if data is None:
                return data
            try:
                if not isinstance(data, str):
                    raise TypeError()
                stale_type_0 = Stale(data)

                return stale_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(None | Stale, data)

        stale = _parse_stale(d.pop("stale"))

        def _parse_source_checked(data: object) -> None | SourceChecked:
            if data is None:
                return data
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                source_checked_type_0 = SourceChecked.from_dict(data)

                return source_checked_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(None | SourceChecked, data)

        source_checked = _parse_source_checked(d.pop("source_checked"))

        _severity = d.pop("severity", UNSET)
        severity: MemorySummarySeverity | Unset
        if isinstance(_severity, Unset):
            severity = UNSET
        else:
            severity = MemorySummarySeverity(_severity)

        rule = d.pop("rule", UNSET)

        _skill = d.pop("skill", UNSET)
        skill: SkillFields | Unset
        if isinstance(_skill, Unset):
            skill = UNSET
        else:
            skill = SkillFields.from_dict(_skill)

        _document = d.pop("document", UNSET)
        document: DocumentFields | Unset
        if isinstance(_document, Unset):
            document = UNSET
        else:
            document = DocumentFields.from_dict(_document)

        _test = d.pop("test", UNSET)
        test: TestFields | Unset
        if isinstance(_test, Unset):
            test = UNSET
        else:
            test = TestFields.from_dict(_test)

        load_warning = d.pop("load_warning", UNSET)

        _scan_findings = d.pop("scan_findings", UNSET)
        scan_findings: list[ScanFinding] | Unset = UNSET
        if _scan_findings is not UNSET:
            scan_findings = []
            for scan_findings_item_data in _scan_findings:
                scan_findings_item = ScanFinding.from_dict(scan_findings_item_data)

                scan_findings.append(scan_findings_item)

        def _parse_principal(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        principal = _parse_principal(d.pop("principal", UNSET))

        def _parse_author_key_id(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        author_key_id = _parse_author_key_id(d.pop("author_key_id", UNSET))

        def _parse_scan_finding(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        scan_finding = _parse_scan_finding(d.pop("scan_finding", UNSET))

        _provenance = d.pop("provenance", UNSET)
        provenance: MemorySummaryProvenance | Unset
        if isinstance(_provenance, Unset):
            provenance = UNSET
        else:
            provenance = MemorySummaryProvenance(_provenance)

        def _parse_simulation_id(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        simulation_id = _parse_simulation_id(d.pop("simulation_id", UNSET))

        self_signed = d.pop("self_signed", UNSET)

        plaintext_digest = d.pop("plaintext_digest", UNSET)

        derives_from = cast(list[str], d.pop("derives_from", UNSET))

        _attachments = d.pop("attachments", UNSET)
        attachments: list[AttachmentDescriptor] | Unset = UNSET
        if _attachments is not UNSET:
            attachments = []
            for attachments_item_data in _attachments:
                attachments_item = AttachmentDescriptor.from_dict(attachments_item_data)

                attachments.append(attachments_item)

        def _parse_approved_at(data: object) -> datetime.datetime | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, str):
                    raise TypeError()
                approved_at_type_0 = datetime.datetime.fromisoformat(data)

                return approved_at_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(datetime.datetime | None | Unset, data)

        approved_at = _parse_approved_at(d.pop("approved_at", UNSET))

        def _parse_approver(data: object) -> Approver | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                approver_type_0 = Approver.from_dict(data)

                return approver_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(Approver | None | Unset, data)

        approver = _parse_approver(d.pop("approver", UNSET))

        def _parse_origin(data: object) -> None | Origin | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                origin_type_0 = Origin.from_dict(data)

                return origin_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(None | Origin | Unset, data)

        origin = _parse_origin(d.pop("origin", UNSET))

        def _parse_valid_from(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        valid_from = _parse_valid_from(d.pop("valid_from", UNSET))

        def _parse_valid_until(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        valid_until = _parse_valid_until(d.pop("valid_until", UNSET))

        format_ = d.pop("format", UNSET)

        def _parse_title(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        title = _parse_title(d.pop("title", UNSET))

        search_text = d.pop("search_text", UNSET)

        replaces_valid_until = d.pop("replaces_valid_until", UNSET)

        text = d.pop("text", UNSET)

        needs_person = d.pop("needs_person", UNSET)

        name = d.pop("name", UNSET)

        description = d.pop("description", UNSET)

        preview = d.pop("preview", UNSET)

        uri = d.pop("uri", UNSET)

        def _parse_prompt(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        prompt = _parse_prompt(d.pop("prompt", UNSET))

        in_force = d.pop("in_force", UNSET)

        applies_here = d.pop("applies_here", UNSET)

        size = d.pop("size", UNSET)

        memory = cls(
            artifact_id=artifact_id,
            claim_key=claim_key,
            version=version,
            tip=tip,
            workspace_id=workspace_id,
            workspace_path=workspace_path,
            kind=kind,
            state=state,
            unreviewed=unreviewed,
            tokens=tokens,
            retirement=retirement,
            replaces=replaces,
            held_expires_at=held_expires_at,
            created_at=created_at,
            promoted_at=promoted_at,
            anchor=anchor,
            stale=stale,
            source_checked=source_checked,
            severity=severity,
            rule=rule,
            skill=skill,
            document=document,
            test=test,
            load_warning=load_warning,
            scan_findings=scan_findings,
            principal=principal,
            author_key_id=author_key_id,
            scan_finding=scan_finding,
            provenance=provenance,
            simulation_id=simulation_id,
            self_signed=self_signed,
            plaintext_digest=plaintext_digest,
            derives_from=derives_from,
            attachments=attachments,
            approved_at=approved_at,
            approver=approver,
            origin=origin,
            valid_from=valid_from,
            valid_until=valid_until,
            format_=format_,
            title=title,
            search_text=search_text,
            replaces_valid_until=replaces_valid_until,
            text=text,
            needs_person=needs_person,
            name=name,
            description=description,
            preview=preview,
            uri=uri,
            prompt=prompt,
            in_force=in_force,
            applies_here=applies_here,
            size=size,
        )

        memory.additional_properties = d
        return memory

    @property
    def additional_keys(self) -> list[str]:
        return list(self.additional_properties.keys())

    def __getitem__(self, key: str) -> Any:
        return self.additional_properties[key]

    def __setitem__(self, key: str, value: Any) -> None:
        self.additional_properties[key] = value

    def __delitem__(self, key: str) -> None:
        del self.additional_properties[key]

    def __contains__(self, key: str) -> bool:
        return key in self.additional_properties
