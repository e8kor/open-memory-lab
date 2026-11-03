from __future__ import annotations

import datetime
from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, Self, TypeVar, cast

from attrs import define as _attrs_define

from ..models.key_issue_request_held_by import KeyIssueRequestHeldBy
from ..models.role import Role
from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.grant_request import GrantRequest


T = TypeVar("T", bound="KeyIssueRequest")


@_attrs_define
class KeyIssueRequest:
    """
    Attributes:
        role (Role):
        name (str): A new name, in a creation body: the creation grammar. A lowercase RFC 1035 label, 1-63 characters,
            that is not in the id form, not 32 hexadecimal digits and not the derived form, which only rows that predate
            names hold; a creation that sends that form is request.validation. Unique in its namespace, immutable, never
            released; compared byte for byte, never lowercased (docs/api/http-api.md, Names and ids; ADR-0026 d1, d2).
            References use NameRef.
        grants (list[GrantRequest] | Unset): Taken exactly as sent: the server adds no grant (ADR-0025 d2). The CLI's
            reader grant beside a writer grant is sent here like any other (ADR-0028 d2).
        default_workarea_id (str | Unset): A workarea, by its node id or its name. Resolved server-side before any check
            that reads state; an unknown name answers exactly what an unknown id answers. A derived path is neither, and is
            refused (request.validation).
        held_by (KeyIssueRequestHeldBy | Unset): The issuing admin's journalled attestation (ADR-0021); never verified
            as fact.
        crew (str | Unset): A crew record, by its id or its name, resolved server-side like every reference (ADR-0026
            d4).
        expiry (datetime.datetime | Unset):
        label (str | Unset): A free-text note; unique nowhere.
        principal (str | Unset): Who acts — the attribution identity
        tenant_id (str | Unset): A tenant, by its id or its name, resolved in the deployment's namespace (ADR-0026 d4).
    """

    role: Role
    name: str
    grants: list[GrantRequest] | Unset = UNSET
    default_workarea_id: str | Unset = UNSET
    held_by: KeyIssueRequestHeldBy | Unset = UNSET
    crew: str | Unset = UNSET
    expiry: datetime.datetime | Unset = UNSET
    label: str | Unset = UNSET
    principal: str | Unset = UNSET
    tenant_id: str | Unset = UNSET

    def to_dict(self) -> dict[str, Any]:
        role = self.role.value

        name = self.name

        grants: list[dict[str, Any]] | Unset = UNSET
        if not isinstance(self.grants, Unset):
            grants = []
            for grants_item_data in self.grants:
                grants_item = grants_item_data.to_dict()
                grants.append(grants_item)

        default_workarea_id: str | Unset
        if isinstance(self.default_workarea_id, Unset):
            default_workarea_id = UNSET
        else:
            default_workarea_id = self.default_workarea_id

        held_by: str | Unset = UNSET
        if not isinstance(self.held_by, Unset):
            held_by = self.held_by.value

        crew: str | Unset
        if isinstance(self.crew, Unset):
            crew = UNSET
        else:
            crew = self.crew

        expiry: str | Unset = UNSET
        if not isinstance(self.expiry, Unset):
            expiry = self.expiry.isoformat()

        label = self.label

        principal = self.principal

        tenant_id: str | Unset
        if isinstance(self.tenant_id, Unset):
            tenant_id = UNSET
        else:
            tenant_id = self.tenant_id

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "role": role,
                "name": name,
            }
        )
        if grants is not UNSET:
            field_dict["grants"] = grants
        if default_workarea_id is not UNSET:
            field_dict["default_workarea_id"] = default_workarea_id
        if held_by is not UNSET:
            field_dict["held_by"] = held_by
        if crew is not UNSET:
            field_dict["crew"] = crew
        if expiry is not UNSET:
            field_dict["expiry"] = expiry
        if label is not UNSET:
            field_dict["label"] = label
        if principal is not UNSET:
            field_dict["principal"] = principal
        if tenant_id is not UNSET:
            field_dict["tenant_id"] = tenant_id

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.grant_request import GrantRequest

        d = dict(src_dict)
        role = Role(d.pop("role"))

        name = d.pop("name")

        _grants = d.pop("grants", UNSET)
        grants: list[GrantRequest] | Unset = UNSET
        if _grants is not UNSET:
            grants = []
            for grants_item_data in _grants:
                grants_item = GrantRequest.from_dict(grants_item_data)

                grants.append(grants_item)

        def _parse_default_workarea_id(data: object) -> str | Unset:
            if isinstance(data, Unset):
                return data
            return cast(str | Unset, data)

        default_workarea_id = _parse_default_workarea_id(
            d.pop("default_workarea_id", UNSET)
        )

        _held_by = d.pop("held_by", UNSET)
        held_by: KeyIssueRequestHeldBy | Unset
        if isinstance(_held_by, Unset):
            held_by = UNSET
        else:
            held_by = KeyIssueRequestHeldBy(_held_by)

        def _parse_crew(data: object) -> str | Unset:
            if isinstance(data, Unset):
                return data
            return cast(str | Unset, data)

        crew = _parse_crew(d.pop("crew", UNSET))

        _expiry = d.pop("expiry", UNSET)
        expiry: datetime.datetime | Unset
        if isinstance(_expiry, Unset):
            expiry = UNSET
        else:
            expiry = datetime.datetime.fromisoformat(_expiry)

        label = d.pop("label", UNSET)

        principal = d.pop("principal", UNSET)

        def _parse_tenant_id(data: object) -> str | Unset:
            if isinstance(data, Unset):
                return data
            return cast(str | Unset, data)

        tenant_id = _parse_tenant_id(d.pop("tenant_id", UNSET))

        key_issue_request = cls(
            role=role,
            name=name,
            grants=grants,
            default_workarea_id=default_workarea_id,
            held_by=held_by,
            crew=crew,
            expiry=expiry,
            label=label,
            principal=principal,
            tenant_id=tenant_id,
        )

        return key_issue_request
