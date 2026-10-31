from __future__ import annotations

import datetime
from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, Self, TypeVar

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
        grants (list[GrantRequest] | Unset):
        default_workarea_id (str | Unset): The provider-ring target (ADR-0022 d4); its validation and its default when
            omitted are declared in the issuance order of docs/architecture/roles-and-keys.md.
        held_by (KeyIssueRequestHeldBy | Unset): The issuing admin's journalled attestation (ADR-0021); never verified
            as fact.
        expiry (datetime.datetime | Unset):
        label (str | Unset):
        principal (str | Unset):
        tenant_id (str | Unset): Operator keys only, for a tenant's first admin key.
    """

    role: Role
    grants: list[GrantRequest] | Unset = UNSET
    default_workarea_id: str | Unset = UNSET
    held_by: KeyIssueRequestHeldBy | Unset = UNSET
    expiry: datetime.datetime | Unset = UNSET
    label: str | Unset = UNSET
    principal: str | Unset = UNSET
    tenant_id: str | Unset = UNSET

    def to_dict(self) -> dict[str, Any]:
        role = self.role.value

        grants: list[dict[str, Any]] | Unset = UNSET
        if not isinstance(self.grants, Unset):
            grants = []
            for grants_item_data in self.grants:
                grants_item = grants_item_data.to_dict()
                grants.append(grants_item)

        default_workarea_id = self.default_workarea_id

        held_by: str | Unset = UNSET
        if not isinstance(self.held_by, Unset):
            held_by = self.held_by.value

        expiry: str | Unset = UNSET
        if not isinstance(self.expiry, Unset):
            expiry = self.expiry.isoformat()

        label = self.label

        principal = self.principal

        tenant_id = self.tenant_id

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "role": role,
            }
        )
        if grants is not UNSET:
            field_dict["grants"] = grants
        if default_workarea_id is not UNSET:
            field_dict["default_workarea_id"] = default_workarea_id
        if held_by is not UNSET:
            field_dict["held_by"] = held_by
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

        _grants = d.pop("grants", UNSET)
        grants: list[GrantRequest] | Unset = UNSET
        if _grants is not UNSET:
            grants = []
            for grants_item_data in _grants:
                grants_item = GrantRequest.from_dict(grants_item_data)

                grants.append(grants_item)

        default_workarea_id = d.pop("default_workarea_id", UNSET)

        _held_by = d.pop("held_by", UNSET)
        held_by: KeyIssueRequestHeldBy | Unset
        if isinstance(_held_by, Unset):
            held_by = UNSET
        else:
            held_by = KeyIssueRequestHeldBy(_held_by)

        _expiry = d.pop("expiry", UNSET)
        expiry: datetime.datetime | Unset
        if isinstance(_expiry, Unset):
            expiry = UNSET
        else:
            expiry = datetime.datetime.fromisoformat(_expiry)

        label = d.pop("label", UNSET)

        principal = d.pop("principal", UNSET)

        tenant_id = d.pop("tenant_id", UNSET)

        key_issue_request = cls(
            role=role,
            grants=grants,
            default_workarea_id=default_workarea_id,
            held_by=held_by,
            expiry=expiry,
            label=label,
            principal=principal,
            tenant_id=tenant_id,
        )

        return key_issue_request
