from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, Self, TypeVar

from attrs import define as _attrs_define

from ..models.guardrail_scope import GuardrailScope

if TYPE_CHECKING:
    from ..models.guardrail_set_own_held_item import GuardrailSetOwnHeldItem
    from ..models.served_guardrail import ServedGuardrail


T = TypeVar("T", bound="GuardrailSet")


@_attrs_define
class GuardrailSet:
    """The guardrails in force at the scope node under the key's reader access and guardrail scope (ADR-0037 d3, d4, d5,
    d7).

        Attributes:
            scope_node (str):
            scope_path (str): The scope node's derived path, root first; the hook's header names it.
            guardrail_scope (GuardrailScope): Which applicable guardrails the key loads: workarea, the scope node's alone;
                ancestors, the node's and every ancestor's the key reads. Set at issuance, ancestors when omitted, journalled in
                key.issued; no request changes it, and any other value is request.validation, details.field /guardrail_scope
                (ADR-0037 d4; since v0.5.1).
            set_digest (str): Opaque; a client only compares it.
            over_cap (bool): More than twelve apply; the set is still served whole.
            guardrails (list[ServedGuardrail]): Ordered by severity, depth from the root, promoted_at and artifact_id; never
                by a query.
            own_held (list[GuardrailSetOwnHeldItem]): The caller's own held guardrails in the chain, by key and version,
                never by text.
    """

    scope_node: str
    scope_path: str
    guardrail_scope: GuardrailScope
    set_digest: str
    over_cap: bool
    guardrails: list[ServedGuardrail]
    own_held: list[GuardrailSetOwnHeldItem]

    def to_dict(self) -> dict[str, Any]:
        scope_node = self.scope_node

        scope_path = self.scope_path

        guardrail_scope = self.guardrail_scope.value

        set_digest = self.set_digest

        over_cap = self.over_cap

        guardrails = []
        for guardrails_item_data in self.guardrails:
            guardrails_item = guardrails_item_data.to_dict()
            guardrails.append(guardrails_item)

        own_held = []
        for own_held_item_data in self.own_held:
            own_held_item = own_held_item_data.to_dict()
            own_held.append(own_held_item)

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "scope_node": scope_node,
                "scope_path": scope_path,
                "guardrail_scope": guardrail_scope,
                "set_digest": set_digest,
                "over_cap": over_cap,
                "guardrails": guardrails,
                "own_held": own_held,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.guardrail_set_own_held_item import GuardrailSetOwnHeldItem
        from ..models.served_guardrail import ServedGuardrail

        d = dict(src_dict)
        scope_node = d.pop("scope_node")

        scope_path = d.pop("scope_path")

        guardrail_scope = GuardrailScope(d.pop("guardrail_scope"))

        set_digest = d.pop("set_digest")

        over_cap = d.pop("over_cap")

        guardrails = []
        _guardrails = d.pop("guardrails")
        for guardrails_item_data in _guardrails:
            guardrails_item = ServedGuardrail.from_dict(guardrails_item_data)

            guardrails.append(guardrails_item)

        own_held = []
        _own_held = d.pop("own_held")
        for own_held_item_data in _own_held:
            own_held_item = GuardrailSetOwnHeldItem.from_dict(own_held_item_data)

            own_held.append(own_held_item)

        guardrail_set = cls(
            scope_node=scope_node,
            scope_path=scope_path,
            guardrail_scope=guardrail_scope,
            set_digest=set_digest,
            over_cap=over_cap,
            guardrails=guardrails,
            own_held=own_held,
        )

        return guardrail_set
