from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.guardrail_rule_effect import GuardrailRuleEffect
from ..models.guardrail_rule_rule import GuardrailRuleRule
from ..models.guardrail_rule_tool import GuardrailRuleTool
from ..types import UNSET, Unset

T = TypeVar("T", bound="GuardrailRule")


@_attrs_define
class GuardrailRule:
    """A guardrail's optional rule in the oml-gr/1 grammar: one tool class, its match and an effect, deny or ask, never
    allow; should takes ask only. Refused on every other kind and on a retirement (request.validation, details.field
    /rule); a rule outside the grammar, or over 160 bytes, is vetoed as submission.scan_veto with
    guardrail.rule_unparsable or guardrail.too_long (ADR-0037 d8, d10; since v0.5.1).

        Attributes:
            rule (GuardrailRuleRule):
            tool (GuardrailRuleTool):
            effect (GuardrailRuleEffect):
            command (list[str] | Unset): shell: word globs matched against each segment of a command.
            path (str | Unset): edit and read: a path glob.
            host (str | Unset): fetch: an exact host or a *. suffix, in its WHATWG serialisation.
            name (str | Unset): mcp: a glob against the tool name.
    """

    rule: GuardrailRuleRule
    tool: GuardrailRuleTool
    effect: GuardrailRuleEffect
    command: list[str] | Unset = UNSET
    path: str | Unset = UNSET
    host: str | Unset = UNSET
    name: str | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        rule = self.rule.value

        tool = self.tool.value

        effect = self.effect.value

        command: list[str] | Unset = UNSET
        if not isinstance(self.command, Unset):
            command = self.command

        path = self.path

        host = self.host

        name = self.name

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "rule": rule,
                "tool": tool,
                "effect": effect,
            }
        )
        if command is not UNSET:
            field_dict["command"] = command
        if path is not UNSET:
            field_dict["path"] = path
        if host is not UNSET:
            field_dict["host"] = host
        if name is not UNSET:
            field_dict["name"] = name

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        rule = GuardrailRuleRule(d.pop("rule"))

        tool = GuardrailRuleTool(d.pop("tool"))

        effect = GuardrailRuleEffect(d.pop("effect"))

        command = cast(list[str], d.pop("command", UNSET))

        path = d.pop("path", UNSET)

        host = d.pop("host", UNSET)

        name = d.pop("name", UNSET)

        guardrail_rule = cls(
            rule=rule,
            tool=tool,
            effect=effect,
            command=command,
            path=path,
            host=host,
            name=name,
        )

        guardrail_rule.additional_properties = d
        return guardrail_rule

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
