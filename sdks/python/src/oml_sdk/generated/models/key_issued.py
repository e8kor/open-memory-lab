from __future__ import annotations

import datetime
from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, Self, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.key_held_by_type_1 import KeyHeldByType1
from ..models.key_held_by_type_2_type_1 import KeyHeldByType2Type1
from ..models.key_held_by_type_3_type_1 import KeyHeldByType3Type1
from ..models.role import Role
from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.denial import Denial
    from ..models.grant import Grant


T = TypeVar("T", bound="KeyIssued")


@_attrs_define
class KeyIssued:
    """
    Attributes:
        key_id (str):
        role (Role):
        default_workarea_id (None | str): The key's default workarea; for an issued key, as key.issued journals it — the
            one sent, else the first grant's node, null for a zero-grant key (docs/architecture/roles-and-keys.md, issuance
            order; ADR-0025 d2). The provider-ring target when no X-OML-Workarea is sent (ADR-0022 d4).
        held_by (KeyHeldByType1 | KeyHeldByType2Type1 | KeyHeldByType3Type1 | None): The issuing admin's attestation;
            for an issued key, as key.issued journals it, null when none was sent (ADR-0025 d2).
        value (str): The secret — returned exactly once, journalled as issued-without-value.
        principal (str | Unset):
        label (str | Unset):
        expiry (datetime.datetime | None | Unset):
        revoked_at (datetime.datetime | None | Unset):
        rotated_from (None | str | Unset): The key_id this key succeeded by rotation
        grants (list[Grant] | Unset):
        denials (list[Denial] | Unset):
        epoch (int | Unset):
    """

    key_id: str
    role: Role
    default_workarea_id: None | str
    held_by: KeyHeldByType1 | KeyHeldByType2Type1 | KeyHeldByType3Type1 | None
    value: str
    principal: str | Unset = UNSET
    label: str | Unset = UNSET
    expiry: datetime.datetime | None | Unset = UNSET
    revoked_at: datetime.datetime | None | Unset = UNSET
    rotated_from: None | str | Unset = UNSET
    grants: list[Grant] | Unset = UNSET
    denials: list[Denial] | Unset = UNSET
    epoch: int | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        key_id = self.key_id

        role = self.role.value

        default_workarea_id: None | str
        default_workarea_id = self.default_workarea_id

        held_by: None | str
        if (
            isinstance(self.held_by, KeyHeldByType1)
            or isinstance(self.held_by, KeyHeldByType2Type1)
            or isinstance(self.held_by, KeyHeldByType3Type1)
        ):
            held_by = self.held_by.value
        else:
            held_by = self.held_by

        value = self.value

        principal = self.principal

        label = self.label

        expiry: None | str | Unset
        if isinstance(self.expiry, Unset):
            expiry = UNSET
        elif isinstance(self.expiry, datetime.datetime):
            expiry = self.expiry.isoformat()
        else:
            expiry = self.expiry

        revoked_at: None | str | Unset
        if isinstance(self.revoked_at, Unset):
            revoked_at = UNSET
        elif isinstance(self.revoked_at, datetime.datetime):
            revoked_at = self.revoked_at.isoformat()
        else:
            revoked_at = self.revoked_at

        rotated_from: None | str | Unset
        if isinstance(self.rotated_from, Unset):
            rotated_from = UNSET
        else:
            rotated_from = self.rotated_from

        grants: list[dict[str, Any]] | Unset = UNSET
        if not isinstance(self.grants, Unset):
            grants = []
            for grants_item_data in self.grants:
                grants_item = grants_item_data.to_dict()
                grants.append(grants_item)

        denials: list[dict[str, Any]] | Unset = UNSET
        if not isinstance(self.denials, Unset):
            denials = []
            for denials_item_data in self.denials:
                denials_item = denials_item_data.to_dict()
                denials.append(denials_item)

        epoch = self.epoch

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "key_id": key_id,
                "role": role,
                "default_workarea_id": default_workarea_id,
                "held_by": held_by,
                "value": value,
            }
        )
        if principal is not UNSET:
            field_dict["principal"] = principal
        if label is not UNSET:
            field_dict["label"] = label
        if expiry is not UNSET:
            field_dict["expiry"] = expiry
        if revoked_at is not UNSET:
            field_dict["revoked_at"] = revoked_at
        if rotated_from is not UNSET:
            field_dict["rotated_from"] = rotated_from
        if grants is not UNSET:
            field_dict["grants"] = grants
        if denials is not UNSET:
            field_dict["denials"] = denials
        if epoch is not UNSET:
            field_dict["epoch"] = epoch

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.denial import Denial
        from ..models.grant import Grant

        d = dict(src_dict)
        key_id = d.pop("key_id")

        role = Role(d.pop("role"))

        def _parse_default_workarea_id(data: object) -> None | str:
            if data is None:
                return data
            return cast(None | str, data)

        default_workarea_id = _parse_default_workarea_id(d.pop("default_workarea_id"))

        def _parse_held_by(
            data: object,
        ) -> KeyHeldByType1 | KeyHeldByType2Type1 | KeyHeldByType3Type1 | None:
            if data is None:
                return data
            try:
                if not isinstance(data, str):
                    raise TypeError()
                held_by_type_1 = KeyHeldByType1(data)

                return held_by_type_1
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            try:
                if not isinstance(data, str):
                    raise TypeError()
                held_by_type_2_type_1 = KeyHeldByType2Type1(data)

                return held_by_type_2_type_1
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            try:
                if not isinstance(data, str):
                    raise TypeError()
                held_by_type_3_type_1 = KeyHeldByType3Type1(data)

                return held_by_type_3_type_1
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(
                KeyHeldByType1 | KeyHeldByType2Type1 | KeyHeldByType3Type1 | None, data
            )

        held_by = _parse_held_by(d.pop("held_by"))

        value = d.pop("value")

        principal = d.pop("principal", UNSET)

        label = d.pop("label", UNSET)

        def _parse_expiry(data: object) -> datetime.datetime | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, str):
                    raise TypeError()
                expiry_type_0 = datetime.datetime.fromisoformat(data)

                return expiry_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(datetime.datetime | None | Unset, data)

        expiry = _parse_expiry(d.pop("expiry", UNSET))

        def _parse_revoked_at(data: object) -> datetime.datetime | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, str):
                    raise TypeError()
                revoked_at_type_0 = datetime.datetime.fromisoformat(data)

                return revoked_at_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(datetime.datetime | None | Unset, data)

        revoked_at = _parse_revoked_at(d.pop("revoked_at", UNSET))

        def _parse_rotated_from(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        rotated_from = _parse_rotated_from(d.pop("rotated_from", UNSET))

        _grants = d.pop("grants", UNSET)
        grants: list[Grant] | Unset = UNSET
        if _grants is not UNSET:
            grants = []
            for grants_item_data in _grants:
                grants_item = Grant.from_dict(grants_item_data)

                grants.append(grants_item)

        _denials = d.pop("denials", UNSET)
        denials: list[Denial] | Unset = UNSET
        if _denials is not UNSET:
            denials = []
            for denials_item_data in _denials:
                denials_item = Denial.from_dict(denials_item_data)

                denials.append(denials_item)

        epoch = d.pop("epoch", UNSET)

        key_issued = cls(
            key_id=key_id,
            role=role,
            default_workarea_id=default_workarea_id,
            held_by=held_by,
            value=value,
            principal=principal,
            label=label,
            expiry=expiry,
            revoked_at=revoked_at,
            rotated_from=rotated_from,
            grants=grants,
            denials=denials,
            epoch=epoch,
        )

        key_issued.additional_properties = d
        return key_issued

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
