from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, Self, TypeVar, cast

from attrs import define as _attrs_define

from ..models.role import Role
from ..models.whoami_held_by import WhoamiHeldBy

if TYPE_CHECKING:
    from ..models.grant import Grant
    from ..models.whoami_ref import WhoamiRef
    from ..models.whoami_workarea import WhoamiWorkarea


T = TypeVar("T", bound="Whoami")


@_attrs_define
class Whoami:
    """The calling key's own record (GET /v2/whoami): everything a client derives from its URL and key alone. Never the
    key's value and never its denials (ADR-0039 d15).

        Attributes:
            key_id (str):
            name (None | str): The key's name; null only for a key that predates names, which the lead names before it
                serves (ADR-0026 d11).
            role (Role):
            principal (str):
            held_by (WhoamiHeldBy): As the key's row stores it: the issuing admin's attestation, agent when none was sent.
            tenant (None | WhoamiRef): The key's tenant; null for an operator key, which no tenant holds.
            default_workarea (None | WhoamiWorkarea): The key's journalled default workarea; null when it has none.
            crew (None | WhoamiRef): The key's crew record, fixed for its life; null when none was named.
            grants (list[Grant]): The key's stored grants, each with its epoch.
    """

    key_id: str
    name: None | str
    role: Role
    principal: str
    held_by: WhoamiHeldBy
    tenant: None | WhoamiRef
    default_workarea: None | WhoamiWorkarea
    crew: None | WhoamiRef
    grants: list[Grant]

    def to_dict(self) -> dict[str, Any]:
        from ..models.whoami_ref import WhoamiRef
        from ..models.whoami_workarea import WhoamiWorkarea

        key_id = self.key_id

        name: None | str
        name = self.name

        role = self.role.value

        principal = self.principal

        held_by = self.held_by.value

        tenant: dict[str, Any] | None
        if isinstance(self.tenant, WhoamiRef):
            tenant = self.tenant.to_dict()
        else:
            tenant = self.tenant

        default_workarea: dict[str, Any] | None
        if isinstance(self.default_workarea, WhoamiWorkarea):
            default_workarea = self.default_workarea.to_dict()
        else:
            default_workarea = self.default_workarea

        crew: dict[str, Any] | None
        if isinstance(self.crew, WhoamiRef):
            crew = self.crew.to_dict()
        else:
            crew = self.crew

        grants = []
        for grants_item_data in self.grants:
            grants_item = grants_item_data.to_dict()
            grants.append(grants_item)

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "key_id": key_id,
                "name": name,
                "role": role,
                "principal": principal,
                "held_by": held_by,
                "tenant": tenant,
                "default_workarea": default_workarea,
                "crew": crew,
                "grants": grants,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.grant import Grant
        from ..models.whoami_ref import WhoamiRef
        from ..models.whoami_workarea import WhoamiWorkarea

        d = dict(src_dict)
        key_id = d.pop("key_id")

        def _parse_name(data: object) -> None | str:
            if data is None:
                return data
            return cast(None | str, data)

        name = _parse_name(d.pop("name"))

        role = Role(d.pop("role"))

        principal = d.pop("principal")

        held_by = WhoamiHeldBy(d.pop("held_by"))

        def _parse_tenant(data: object) -> None | WhoamiRef:
            if data is None:
                return data
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                tenant_type_0 = WhoamiRef.from_dict(data)

                return tenant_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(None | WhoamiRef, data)

        tenant = _parse_tenant(d.pop("tenant"))

        def _parse_default_workarea(data: object) -> None | WhoamiWorkarea:
            if data is None:
                return data
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                default_workarea_type_0 = WhoamiWorkarea.from_dict(data)

                return default_workarea_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(None | WhoamiWorkarea, data)

        default_workarea = _parse_default_workarea(d.pop("default_workarea"))

        def _parse_crew(data: object) -> None | WhoamiRef:
            if data is None:
                return data
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                crew_type_0 = WhoamiRef.from_dict(data)

                return crew_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(None | WhoamiRef, data)

        crew = _parse_crew(d.pop("crew"))

        grants = []
        _grants = d.pop("grants")
        for grants_item_data in _grants:
            grants_item = Grant.from_dict(grants_item_data)

            grants.append(grants_item)

        whoami = cls(
            key_id=key_id,
            name=name,
            role=role,
            principal=principal,
            held_by=held_by,
            tenant=tenant,
            default_workarea=default_workarea,
            crew=crew,
            grants=grants,
        )

        return whoami
