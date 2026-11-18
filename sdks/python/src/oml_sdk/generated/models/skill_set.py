from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, Self, TypeVar, cast

from attrs import define as _attrs_define

from ..models.guardrail_scope import GuardrailScope

if TYPE_CHECKING:
    from ..models.served_skill import ServedSkill


T = TypeVar("T", bound="SkillSet")


@_attrs_define
class SkillSet:
    """One page of the skills offered at the scope node, without bodies (ADR-0080 d11).

    Attributes:
        scope_node (str):
        scope_path (str): The scope node's derived path, root first.
        guardrail_scope (GuardrailScope): Which applicable guardrails the key loads: workspace, the scope node's alone;
            ancestors, the node's and every ancestor's the key reads. Set at issuance, ancestors when omitted, journalled in
            key.issued; no request changes it, and any other value is request.validation, details.field /guardrail_scope
            (ADR-0037 d4; since v0.5.1).
        set_digest (str): Opaque, over the whole offered set, every page alike; a client only compares it.
        skills (list[ServedSkill]): Ordered by depth from the scope node, then name.
        next_cursor (None | str): Null on the last page.
    """

    scope_node: str
    scope_path: str
    guardrail_scope: GuardrailScope
    set_digest: str
    skills: list[ServedSkill]
    next_cursor: None | str

    def to_dict(self) -> dict[str, Any]:
        scope_node = self.scope_node

        scope_path = self.scope_path

        guardrail_scope = self.guardrail_scope.value

        set_digest = self.set_digest

        skills = []
        for skills_item_data in self.skills:
            skills_item = skills_item_data.to_dict()
            skills.append(skills_item)

        next_cursor: None | str
        next_cursor = self.next_cursor

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "scope_node": scope_node,
                "scope_path": scope_path,
                "guardrail_scope": guardrail_scope,
                "set_digest": set_digest,
                "skills": skills,
                "next_cursor": next_cursor,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.served_skill import ServedSkill

        d = dict(src_dict)
        scope_node = d.pop("scope_node")

        scope_path = d.pop("scope_path")

        guardrail_scope = GuardrailScope(d.pop("guardrail_scope"))

        set_digest = d.pop("set_digest")

        skills = []
        _skills = d.pop("skills")
        for skills_item_data in _skills:
            skills_item = ServedSkill.from_dict(skills_item_data)

            skills.append(skills_item)

        def _parse_next_cursor(data: object) -> None | str:
            if data is None:
                return data
            return cast(None | str, data)

        next_cursor = _parse_next_cursor(d.pop("next_cursor"))

        skill_set = cls(
            scope_node=scope_node,
            scope_path=scope_path,
            guardrail_scope=guardrail_scope,
            set_digest=set_digest,
            skills=skills,
            next_cursor=next_cursor,
        )

        return skill_set
