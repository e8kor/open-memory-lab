from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, Self, TypeVar, cast

from attrs import define as _attrs_define

from ..models.guardrail_scope import GuardrailScope
from ..models.role import Role
from ..models.whoami_held_by import WhoamiHeldBy
from ..models.whoami_profile_type_1 import WhoamiProfileType1
from ..models.whoami_profile_type_2_type_1 import WhoamiProfileType2Type1
from ..models.whoami_profile_type_3_type_1 import WhoamiProfileType3Type1
from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.whoami_grant import WhoamiGrant
    from ..models.whoami_person import WhoamiPerson
    from ..models.whoami_ref import WhoamiRef
    from ..models.whoami_workarea import WhoamiWorkarea


T = TypeVar("T", bound="Whoami")


@_attrs_define
class Whoami:
    """The calling key's own record (GET /v2/whoami): everything a client derives from its URL and key alone. Never the
    key's value, its denials or its grants' deny masks (ADR-0039 d15).

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
            user (None | WhoamiRef): The key's user root, from which a client learns its root; null for a tenant key
                (ADR-0044 d11).
            grants (list[WhoamiGrant]): The key's stored grants, each with its epoch and without its deny masks.
            guardrail_scope (GuardrailScope | Unset): Which applicable guardrails the key loads: workarea, the scope node's
                alone; ancestors, the node's and every ancestor's the key reads. Set at issuance, ancestors when omitted,
                journalled in key.issued; no request changes it, and any other value is request.validation, details.field
                /guardrail_scope (ADR-0037 d4; since v0.5.1).
            profile (None | Unset | WhoamiProfileType1 | WhoamiProfileType2Type1 | WhoamiProfileType3Type1): The key's
                profile; null for none (ADR-0030 d8).
            person (None | Unset | WhoamiPerson): The person the key's user root names; null for a tenant key (ADR-0070 d5).
                Absent with person=false (#286).
            shared (bool | Unset): Whether the server's admins are not the caller's: true when the operator sets
                server.shared or opens sign-up. oml sync asks consent of a shared server; a server from this release always
                sends it; optional here so a client reads an answer without it, from an older server, as false (ADR-0076 d9).
    """

    key_id: str
    name: None | str
    role: Role
    principal: str
    held_by: WhoamiHeldBy
    tenant: None | WhoamiRef
    default_workarea: None | WhoamiWorkarea
    crew: None | WhoamiRef
    user: None | WhoamiRef
    grants: list[WhoamiGrant]
    guardrail_scope: GuardrailScope | Unset = UNSET
    profile: (
        None
        | Unset
        | WhoamiProfileType1
        | WhoamiProfileType2Type1
        | WhoamiProfileType3Type1
    ) = UNSET
    person: None | Unset | WhoamiPerson = UNSET
    shared: bool | Unset = UNSET

    def to_dict(self) -> dict[str, Any]:
        from ..models.whoami_person import WhoamiPerson
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

        user: dict[str, Any] | None
        if isinstance(self.user, WhoamiRef):
            user = self.user.to_dict()
        else:
            user = self.user

        grants = []
        for grants_item_data in self.grants:
            grants_item = grants_item_data.to_dict()
            grants.append(grants_item)

        guardrail_scope: str | Unset = UNSET
        if not isinstance(self.guardrail_scope, Unset):
            guardrail_scope = self.guardrail_scope.value

        profile: None | str | Unset
        if isinstance(self.profile, Unset):
            profile = UNSET
        elif (
            isinstance(self.profile, WhoamiProfileType1)
            or isinstance(self.profile, WhoamiProfileType2Type1)
            or isinstance(self.profile, WhoamiProfileType3Type1)
        ):
            profile = self.profile.value
        else:
            profile = self.profile

        person: dict[str, Any] | None | Unset
        if isinstance(self.person, Unset):
            person = UNSET
        elif isinstance(self.person, WhoamiPerson):
            person = self.person.to_dict()
        else:
            person = self.person

        shared = self.shared

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
                "user": user,
                "grants": grants,
            }
        )
        if guardrail_scope is not UNSET:
            field_dict["guardrail_scope"] = guardrail_scope
        if profile is not UNSET:
            field_dict["profile"] = profile
        if person is not UNSET:
            field_dict["person"] = person
        if shared is not UNSET:
            field_dict["shared"] = shared

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.whoami_grant import WhoamiGrant
        from ..models.whoami_person import WhoamiPerson
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

        def _parse_user(data: object) -> None | WhoamiRef:
            if data is None:
                return data
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                user_type_0 = WhoamiRef.from_dict(data)

                return user_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(None | WhoamiRef, data)

        user = _parse_user(d.pop("user"))

        grants = []
        _grants = d.pop("grants")
        for grants_item_data in _grants:
            grants_item = WhoamiGrant.from_dict(grants_item_data)

            grants.append(grants_item)

        _guardrail_scope = d.pop("guardrail_scope", UNSET)
        guardrail_scope: GuardrailScope | Unset
        if isinstance(_guardrail_scope, Unset):
            guardrail_scope = UNSET
        else:
            guardrail_scope = GuardrailScope(_guardrail_scope)

        def _parse_profile(
            data: object,
        ) -> (
            None
            | Unset
            | WhoamiProfileType1
            | WhoamiProfileType2Type1
            | WhoamiProfileType3Type1
        ):
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, str):
                    raise TypeError()
                profile_type_1 = WhoamiProfileType1(data)

                return profile_type_1
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            try:
                if not isinstance(data, str):
                    raise TypeError()
                profile_type_2_type_1 = WhoamiProfileType2Type1(data)

                return profile_type_2_type_1
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            try:
                if not isinstance(data, str):
                    raise TypeError()
                profile_type_3_type_1 = WhoamiProfileType3Type1(data)

                return profile_type_3_type_1
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(
                None
                | Unset
                | WhoamiProfileType1
                | WhoamiProfileType2Type1
                | WhoamiProfileType3Type1,
                data,
            )

        profile = _parse_profile(d.pop("profile", UNSET))

        def _parse_person(data: object) -> None | Unset | WhoamiPerson:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                person_type_0 = WhoamiPerson.from_dict(data)

                return person_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(None | Unset | WhoamiPerson, data)

        person = _parse_person(d.pop("person", UNSET))

        shared = d.pop("shared", UNSET)

        whoami = cls(
            key_id=key_id,
            name=name,
            role=role,
            principal=principal,
            held_by=held_by,
            tenant=tenant,
            default_workarea=default_workarea,
            crew=crew,
            user=user,
            grants=grants,
            guardrail_scope=guardrail_scope,
            profile=profile,
            person=person,
            shared=shared,
        )

        return whoami
