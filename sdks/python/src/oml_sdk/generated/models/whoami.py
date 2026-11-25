from __future__ import annotations

import datetime
from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, Self, TypeVar, cast

from attrs import define as _attrs_define

from ..models.guardrail_scope import GuardrailScope
from ..models.role import Role
from ..models.whoami_profile_type_1 import WhoamiProfileType1
from ..models.whoami_profile_type_2_type_1 import WhoamiProfileType2Type1
from ..models.whoami_profile_type_3_type_1 import WhoamiProfileType3Type1
from ..models.whoami_used_by import WhoamiUsedBy
from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.whoami_grant import WhoamiGrant
    from ..models.whoami_person import WhoamiPerson
    from ..models.whoami_ref import WhoamiRef
    from ..models.whoami_workspace import WhoamiWorkspace


T = TypeVar("T", bound="Whoami")


@_attrs_define
class Whoami:
    """The calling key's own record (GET /v2/whoami): everything a client derives from its URL and key alone. Never the
    key's value, its denials or its grants' deny masks (ADR-0039 d15); the record of a person's <user>-web session key
    alone carries denied, whether it holds a denial.

        Attributes:
            key_id (str):
            name (None | str): The key's name; null only for a key that predates names, which the lead names before it
                serves (ADR-0026 d11).
            role (Role):
            principal (str):
            used_by (WhoamiUsedBy): As the key's row stores it: the issuing admin's attestation, agent when none was sent.
            tenant (None | WhoamiRef): The key's tenant; null for an operator key, which no tenant holds.
            default_workspace (None | WhoamiWorkspace): The key's journalled default workspace; null when it has none.
            crew (None | WhoamiRef): The key's crew record, fixed for its life; null when none was named.
            user (None | WhoamiRef): The key's user root, from which a client learns its root; null for a tenant key
                (ADR-0044 d11).
            grants (list[WhoamiGrant]): The key's stored grants, each with its epoch and without its deny masks.
            guardrail_scope (GuardrailScope | Unset): Which applicable guardrails the key loads: workspace, the scope node's
                alone; ancestors, the node's and every ancestor's the key reads. Set at issuance, ancestors when omitted,
                journalled in key.issued; no request changes it, and any other value is request.validation, details.field
                /guardrail_scope (ADR-0037 d4; since v0.5.1).
            profile (None | Unset | WhoamiProfileType1 | WhoamiProfileType2Type1 | WhoamiProfileType3Type1): The key's
                profile; null for none (ADR-0030 d8).
            expiry (datetime.datetime | None | Unset): The instant from which this key authenticates nothing; null for a key
                that does not expire (ADR-0079 d3; since v0.7.3).
            requests_per_minute (int | None | Unset): This key's own requests per UTC minute; null for none of its own
                (ADR-0079 d4; since v0.7.3).
            denied (bool | Unset): On the record of a person's <user>-web session key alone, absent on every other key's,
                the other session keys' included: whether the key carries a denial in force on any workspace, which refuses it
                the three key acts (issue a key, mint a connect code, cut off a key) for good, as access.verb_ceiling with
                details.reason session_key_denied. It names no workspace, count, author or time. The web console draws no form
                for those acts when it is true (ADR-0070 d2, d5; ADR-0039 d15; since v0.7.3).
            person (None | Unset | WhoamiPerson): The person the key's user root names; null for a tenant key (ADR-0070 d5).
                Absent with person=false (#286).
            without_recall (bool | Unset): Whether this key is a contributor without recall: a contributor key holding a
                submitter grant on a node that no reader grant of it reaches, before its deny masks, such as a v0.8.3 writer key
                issued write-only, user keys whose one reader grant is the universe node grant among them. It works as before;
                oml doctor names it key_contributor_without_recall. false for every other key, and for every session key the
                server holds for a person, <user>-curate included; optional here so a client reads an answer without it, from an
                older server, as false (ADR-0093 d3, as read on 2026-11-20).
            rotation_grace_until (datetime.datetime | Unset): For a bearer key rotated with a grace still open: when its
                previous value stops authenticating; oml doctor prints key.rotation_grace_open. Absent otherwise (ADR-0096 d13).
                Since v0.8.5.
            shared (bool | Unset): Whether the server's admins are not the caller's: true when the operator sets
                server.shared or opens sign-up. oml sync asks consent of a shared server; a server from this release always
                sends it; optional here so a client reads an answer without it, from an older server, as false (ADR-0076 d9).
    """

    key_id: str
    name: None | str
    role: Role
    principal: str
    used_by: WhoamiUsedBy
    tenant: None | WhoamiRef
    default_workspace: None | WhoamiWorkspace
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
    expiry: datetime.datetime | None | Unset = UNSET
    requests_per_minute: int | None | Unset = UNSET
    denied: bool | Unset = UNSET
    person: None | Unset | WhoamiPerson = UNSET
    without_recall: bool | Unset = UNSET
    rotation_grace_until: datetime.datetime | Unset = UNSET
    shared: bool | Unset = UNSET

    def to_dict(self) -> dict[str, Any]:
        from ..models.whoami_person import WhoamiPerson
        from ..models.whoami_ref import WhoamiRef
        from ..models.whoami_workspace import WhoamiWorkspace

        key_id = self.key_id

        name: None | str
        name = self.name

        role = self.role.value

        principal = self.principal

        used_by = self.used_by.value

        tenant: dict[str, Any] | None
        if isinstance(self.tenant, WhoamiRef):
            tenant = self.tenant.to_dict()
        else:
            tenant = self.tenant

        default_workspace: dict[str, Any] | None
        if isinstance(self.default_workspace, WhoamiWorkspace):
            default_workspace = self.default_workspace.to_dict()
        else:
            default_workspace = self.default_workspace

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

        expiry: None | str | Unset
        if isinstance(self.expiry, Unset):
            expiry = UNSET
        elif isinstance(self.expiry, datetime.datetime):
            expiry = self.expiry.isoformat()
        else:
            expiry = self.expiry

        requests_per_minute: int | None | Unset
        if isinstance(self.requests_per_minute, Unset):
            requests_per_minute = UNSET
        else:
            requests_per_minute = self.requests_per_minute

        denied = self.denied

        person: dict[str, Any] | None | Unset
        if isinstance(self.person, Unset):
            person = UNSET
        elif isinstance(self.person, WhoamiPerson):
            person = self.person.to_dict()
        else:
            person = self.person

        without_recall = self.without_recall

        rotation_grace_until: str | Unset = UNSET
        if not isinstance(self.rotation_grace_until, Unset):
            rotation_grace_until = self.rotation_grace_until.isoformat()

        shared = self.shared

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "key_id": key_id,
                "name": name,
                "role": role,
                "principal": principal,
                "used_by": used_by,
                "tenant": tenant,
                "default_workspace": default_workspace,
                "crew": crew,
                "user": user,
                "grants": grants,
            }
        )
        if guardrail_scope is not UNSET:
            field_dict["guardrail_scope"] = guardrail_scope
        if profile is not UNSET:
            field_dict["profile"] = profile
        if expiry is not UNSET:
            field_dict["expiry"] = expiry
        if requests_per_minute is not UNSET:
            field_dict["requests_per_minute"] = requests_per_minute
        if denied is not UNSET:
            field_dict["denied"] = denied
        if person is not UNSET:
            field_dict["person"] = person
        if without_recall is not UNSET:
            field_dict["without_recall"] = without_recall
        if rotation_grace_until is not UNSET:
            field_dict["rotation_grace_until"] = rotation_grace_until
        if shared is not UNSET:
            field_dict["shared"] = shared

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.whoami_grant import WhoamiGrant
        from ..models.whoami_person import WhoamiPerson
        from ..models.whoami_ref import WhoamiRef
        from ..models.whoami_workspace import WhoamiWorkspace

        d = dict(src_dict)
        key_id = d.pop("key_id")

        def _parse_name(data: object) -> None | str:
            if data is None:
                return data
            return cast(None | str, data)

        name = _parse_name(d.pop("name"))

        role = Role(d.pop("role"))

        principal = d.pop("principal")

        used_by = WhoamiUsedBy(d.pop("used_by"))

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

        def _parse_default_workspace(data: object) -> None | WhoamiWorkspace:
            if data is None:
                return data
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                default_workspace_type_0 = WhoamiWorkspace.from_dict(data)

                return default_workspace_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(None | WhoamiWorkspace, data)

        default_workspace = _parse_default_workspace(d.pop("default_workspace"))

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

        def _parse_requests_per_minute(data: object) -> int | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(int | None | Unset, data)

        requests_per_minute = _parse_requests_per_minute(
            d.pop("requests_per_minute", UNSET)
        )

        denied = d.pop("denied", UNSET)

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

        without_recall = d.pop("without_recall", UNSET)

        _rotation_grace_until = d.pop("rotation_grace_until", UNSET)
        rotation_grace_until: datetime.datetime | Unset
        if isinstance(_rotation_grace_until, Unset):
            rotation_grace_until = UNSET
        else:
            rotation_grace_until = datetime.datetime.fromisoformat(
                _rotation_grace_until
            )

        shared = d.pop("shared", UNSET)

        whoami = cls(
            key_id=key_id,
            name=name,
            role=role,
            principal=principal,
            used_by=used_by,
            tenant=tenant,
            default_workspace=default_workspace,
            crew=crew,
            user=user,
            grants=grants,
            guardrail_scope=guardrail_scope,
            profile=profile,
            expiry=expiry,
            requests_per_minute=requests_per_minute,
            denied=denied,
            person=person,
            without_recall=without_recall,
            rotation_grace_until=rotation_grace_until,
            shared=shared,
        )

        return whoami
