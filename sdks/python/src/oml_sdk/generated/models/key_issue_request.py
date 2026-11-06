from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, Self, TypeVar, cast

from attrs import define as _attrs_define

from ..models.guardrail_scope import GuardrailScope
from ..models.key_issue_request_held_by import KeyIssueRequestHeldBy
from ..models.key_issue_request_profile import KeyIssueRequestProfile
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
        grants (list[GrantRequest] | Unset): Taken exactly as sent, except that a user key also gets a reader grant on
            the universe node alone (ADR-0025 d2; ADR-0044 d3). The CLI's reader grant beside a writer grant is sent here
            like any other (ADR-0028 d2).
        user (str | Unset): A new name, in a creation body: the creation grammar. A lowercase RFC 1035 label, 1-63
            characters, that is not in the id form, not 32 hexadecimal digits and not the derived form, which only rows that
            predate names hold; a creation that sends that form is request.validation. Unique in its namespace, immutable,
            never released; compared byte for byte, never lowercased (docs/api/http-api.md, Names and ids; ADR-0026 d1, d2).
            References use NameRef.
        default_workarea (str | Unset): A workarea, by its node id, its name or its qualified name. Resolved server-side
            before any check that reads state; an unknown name answers exactly what an unknown id answers. A user key
            resolves a bare name in its own user root's namespace, plus universe and its root's name; every other key in the
            shared namespace (ADR-0044 d5). A derived path, a value of three or more labels or one whose first label is
            universe, is neither, and is refused (request.validation).
        held_by (KeyIssueRequestHeldBy | Unset): The issuing admin's journalled attestation (ADR-0021); never verified
            as fact.
        crew (str | Unset): A crew record, by its id or its name, resolved server-side like every reference (ADR-0026
            d4).
        label (str | Unset): A free-text note; unique nowhere.
        principal (str | Unset): Who acts — the attribution identity, shared across one identity's keys; free text.
        guardrail_scope (GuardrailScope | Unset): Which applicable guardrails the key loads: workarea, the scope node's
            alone; ancestors, the node's and every ancestor's the key reads. Set at issuance, ancestors when omitted,
            journalled in key.issued; no request changes it, and any other value is request.validation, details.field
            /guardrail_scope (ADR-0037 d4; since v0.5.1).
        profile (KeyIssueRequestProfile | Unset): curator: a dreamer key held by an agent that may reject, propose
            retirements and summaries, and flag contradictions inside an in-progress workarea only, and never approves; any
            other role or held_by is request.validation, details.field /profile (ADR-0030 d8, d9).
        connect_code (bool | Unset): Issue a connector key (ADR-0031 d7): no bearer value is minted, and the 201 carries
            connect_code in place of value. Refused with 422 oauth.connect_refused for an admin or operator role (role),
            held_by service (held_by_service), or a deployment without oauth.issuer (not_configured). Default: False.
    """

    role: Role
    name: str
    grants: list[GrantRequest] | Unset = UNSET
    user: str | Unset = UNSET
    default_workarea: str | Unset = UNSET
    held_by: KeyIssueRequestHeldBy | Unset = UNSET
    crew: str | Unset = UNSET
    label: str | Unset = UNSET
    principal: str | Unset = UNSET
    guardrail_scope: GuardrailScope | Unset = UNSET
    profile: KeyIssueRequestProfile | Unset = UNSET
    connect_code: bool | Unset = False

    def to_dict(self) -> dict[str, Any]:
        role = self.role.value

        name = self.name

        grants: list[dict[str, Any]] | Unset = UNSET
        if not isinstance(self.grants, Unset):
            grants = []
            for grants_item_data in self.grants:
                grants_item = grants_item_data.to_dict()
                grants.append(grants_item)

        user = self.user

        default_workarea: str | Unset
        if isinstance(self.default_workarea, Unset):
            default_workarea = UNSET
        else:
            default_workarea = self.default_workarea

        held_by: str | Unset = UNSET
        if not isinstance(self.held_by, Unset):
            held_by = self.held_by.value

        crew: str | Unset
        if isinstance(self.crew, Unset):
            crew = UNSET
        else:
            crew = self.crew

        label = self.label

        principal = self.principal

        guardrail_scope: str | Unset = UNSET
        if not isinstance(self.guardrail_scope, Unset):
            guardrail_scope = self.guardrail_scope.value

        profile: str | Unset = UNSET
        if not isinstance(self.profile, Unset):
            profile = self.profile.value

        connect_code = self.connect_code

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "role": role,
                "name": name,
            }
        )
        if grants is not UNSET:
            field_dict["grants"] = grants
        if user is not UNSET:
            field_dict["user"] = user
        if default_workarea is not UNSET:
            field_dict["default_workarea"] = default_workarea
        if held_by is not UNSET:
            field_dict["held_by"] = held_by
        if crew is not UNSET:
            field_dict["crew"] = crew
        if label is not UNSET:
            field_dict["label"] = label
        if principal is not UNSET:
            field_dict["principal"] = principal
        if guardrail_scope is not UNSET:
            field_dict["guardrail_scope"] = guardrail_scope
        if profile is not UNSET:
            field_dict["profile"] = profile
        if connect_code is not UNSET:
            field_dict["connect_code"] = connect_code

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

        user = d.pop("user", UNSET)

        def _parse_default_workarea(data: object) -> str | Unset:
            if isinstance(data, Unset):
                return data
            return cast(str | Unset, data)

        default_workarea = _parse_default_workarea(d.pop("default_workarea", UNSET))

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

        label = d.pop("label", UNSET)

        principal = d.pop("principal", UNSET)

        _guardrail_scope = d.pop("guardrail_scope", UNSET)
        guardrail_scope: GuardrailScope | Unset
        if isinstance(_guardrail_scope, Unset):
            guardrail_scope = UNSET
        else:
            guardrail_scope = GuardrailScope(_guardrail_scope)

        _profile = d.pop("profile", UNSET)
        profile: KeyIssueRequestProfile | Unset
        if isinstance(_profile, Unset):
            profile = UNSET
        else:
            profile = KeyIssueRequestProfile(_profile)

        connect_code = d.pop("connect_code", UNSET)

        key_issue_request = cls(
            role=role,
            name=name,
            grants=grants,
            user=user,
            default_workarea=default_workarea,
            held_by=held_by,
            crew=crew,
            label=label,
            principal=principal,
            guardrail_scope=guardrail_scope,
            profile=profile,
            connect_code=connect_code,
        )

        return key_issue_request
