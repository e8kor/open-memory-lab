from __future__ import annotations

import datetime
from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, Self, TypeVar, cast

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.workarea_status import WorkareaStatus
from ..models.workarea_view_my_effective_verbs_item import (
    WorkareaViewMyEffectiveVerbsItem,
)
from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.brief import Brief
    from ..models.crew import Crew
    from ..models.review_policy import ReviewPolicy
    from ..models.transition import Transition
    from ..models.workarea_view_children_item import WorkareaViewChildrenItem
    from ..models.workarea_view_edges_item import WorkareaViewEdgesItem


T = TypeVar("T", bound="WorkareaView")


@_attrs_define
class WorkareaView:
    """One node with the calling key's view of it; every value below is relative to that key, and children outside read
    access are absent (ADR-0025 d14).

        Attributes:
            id (str):
            name (str): Unique in the tenant and never changed; universe for the root (ADR-0026 d1, d6).
            parent_id (None | str): Null only for the tenant universe root.
            path (str): Derived ltree path, the names from universe joined with "."; never the grant target, and never
                accepted as a reference.
            status (WorkareaStatus):
            children (list[WorkareaViewChildrenItem]): The readable immediate children, ordered by path_leaf ascending in
                byte order, then id; a child's path_leaf is its name.
            edges (list[WorkareaViewEdgesItem]): The workarea edges touching the node whose other endpoint the key can read,
                in either direction, ordered by edge_id ascending; edge_id is the id the events tail reports.
            my_effective_verbs (list[WorkareaViewMyEffectiveVerbsItem]): The effective-verbs table of
                docs/architecture/roles-and-keys.md, unioned over the grant roles effective on the node and listed in table
                order (the reader, writer and dreamer rows, each left to right) without duplicates; the access conferred, not
                what this server build serves.
            permission_epoch (int): The calling key's permission epoch as declared in docs/architecture/workareas.md
                (ADR-0025 d14).
            policy (ReviewPolicy): The review policy in force, the one a transition's policy_snapshot records (ADR-0009 d3).
            pending_transitions (list[Transition]): The node's live proposals in proposal order, the pending rows whose from
                is the node's status (ADR-0039 d9; ADR-0046 d3).
            brief (Brief): The workarea's ratified brief as this key reads it, in ratification order, cut by
                brief.max_tokens; empty for an admin key (ADR-0046 d8, d9).
            crew (Crew | None | Unset): The crew record the workarea names, or null.
            user_root (bool | Unset): Whether the node is a user root, universe.<user>, which only the issuance of the first
                key naming its user creates (ADR-0044 d1, d11).
            created_at (datetime.datetime | Unset):
    """

    id: str
    name: str
    parent_id: None | str
    path: str
    status: WorkareaStatus
    children: list[WorkareaViewChildrenItem]
    edges: list[WorkareaViewEdgesItem]
    my_effective_verbs: list[WorkareaViewMyEffectiveVerbsItem]
    permission_epoch: int
    policy: ReviewPolicy
    pending_transitions: list[Transition]
    brief: Brief
    crew: Crew | None | Unset = UNSET
    user_root: bool | Unset = UNSET
    created_at: datetime.datetime | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        from ..models.crew import Crew

        id = self.id

        name = self.name

        parent_id: None | str
        parent_id = self.parent_id

        path = self.path

        status = self.status.value

        children = []
        for children_item_data in self.children:
            children_item = children_item_data.to_dict()
            children.append(children_item)

        edges = []
        for edges_item_data in self.edges:
            edges_item = edges_item_data.to_dict()
            edges.append(edges_item)

        my_effective_verbs = []
        for my_effective_verbs_item_data in self.my_effective_verbs:
            my_effective_verbs_item = my_effective_verbs_item_data.value
            my_effective_verbs.append(my_effective_verbs_item)

        permission_epoch = self.permission_epoch

        policy = self.policy.to_dict()

        pending_transitions = []
        for pending_transitions_item_data in self.pending_transitions:
            pending_transitions_item = pending_transitions_item_data.to_dict()
            pending_transitions.append(pending_transitions_item)

        brief = self.brief.to_dict()

        crew: dict[str, Any] | None | Unset
        if isinstance(self.crew, Unset):
            crew = UNSET
        elif isinstance(self.crew, Crew):
            crew = self.crew.to_dict()
        else:
            crew = self.crew

        user_root = self.user_root

        created_at: str | Unset = UNSET
        if not isinstance(self.created_at, Unset):
            created_at = self.created_at.isoformat()

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "id": id,
                "name": name,
                "parent_id": parent_id,
                "path": path,
                "status": status,
                "children": children,
                "edges": edges,
                "my_effective_verbs": my_effective_verbs,
                "permission_epoch": permission_epoch,
                "policy": policy,
                "pending_transitions": pending_transitions,
                "brief": brief,
            }
        )
        if crew is not UNSET:
            field_dict["crew"] = crew
        if user_root is not UNSET:
            field_dict["user_root"] = user_root
        if created_at is not UNSET:
            field_dict["created_at"] = created_at

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.brief import Brief
        from ..models.crew import Crew
        from ..models.review_policy import ReviewPolicy
        from ..models.transition import Transition
        from ..models.workarea_view_children_item import WorkareaViewChildrenItem
        from ..models.workarea_view_edges_item import WorkareaViewEdgesItem

        d = dict(src_dict)
        id = d.pop("id")

        name = d.pop("name")

        def _parse_parent_id(data: object) -> None | str:
            if data is None:
                return data
            return cast(None | str, data)

        parent_id = _parse_parent_id(d.pop("parent_id"))

        path = d.pop("path")

        status = WorkareaStatus(d.pop("status"))

        children = []
        _children = d.pop("children")
        for children_item_data in _children:
            children_item = WorkareaViewChildrenItem.from_dict(children_item_data)

            children.append(children_item)

        edges = []
        _edges = d.pop("edges")
        for edges_item_data in _edges:
            edges_item = WorkareaViewEdgesItem.from_dict(edges_item_data)

            edges.append(edges_item)

        my_effective_verbs = []
        _my_effective_verbs = d.pop("my_effective_verbs")
        for my_effective_verbs_item_data in _my_effective_verbs:
            my_effective_verbs_item = WorkareaViewMyEffectiveVerbsItem(
                my_effective_verbs_item_data
            )

            my_effective_verbs.append(my_effective_verbs_item)

        permission_epoch = d.pop("permission_epoch")

        policy = ReviewPolicy.from_dict(d.pop("policy"))

        pending_transitions = []
        _pending_transitions = d.pop("pending_transitions")
        for pending_transitions_item_data in _pending_transitions:
            pending_transitions_item = Transition.from_dict(
                pending_transitions_item_data
            )

            pending_transitions.append(pending_transitions_item)

        brief = Brief.from_dict(d.pop("brief"))

        def _parse_crew(data: object) -> Crew | None | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            try:
                if not isinstance(data, dict):
                    raise TypeError()
                crew_type_0 = Crew.from_dict(data)

                return crew_type_0
            except (TypeError, ValueError, AttributeError, KeyError):
                pass
            return cast(Crew | None | Unset, data)

        crew = _parse_crew(d.pop("crew", UNSET))

        user_root = d.pop("user_root", UNSET)

        _created_at = d.pop("created_at", UNSET)
        created_at: datetime.datetime | Unset
        if isinstance(_created_at, Unset):
            created_at = UNSET
        else:
            created_at = datetime.datetime.fromisoformat(_created_at)

        workarea_view = cls(
            id=id,
            name=name,
            parent_id=parent_id,
            path=path,
            status=status,
            children=children,
            edges=edges,
            my_effective_verbs=my_effective_verbs,
            permission_epoch=permission_epoch,
            policy=policy,
            pending_transitions=pending_transitions,
            brief=brief,
            crew=crew,
            user_root=user_root,
            created_at=created_at,
        )

        workarea_view.additional_properties = d
        return workarea_view

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
