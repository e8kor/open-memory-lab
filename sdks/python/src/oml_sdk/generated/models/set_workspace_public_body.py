from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar, cast

from attrs import define as _attrs_define

from ..types import UNSET, Unset

T = TypeVar("T", bound="SetWorkspacePublicBody")


@_attrs_define
class SetWorkspacePublicBody:
    """
    Attributes:
        public (bool): true sets the flag, false clears it.
        reason (None | str | Unset): Why; required for a set outside sole_admin, scanned as a verdict's reason is and
            journalled with the admin act (ADR-0097 d26).
    """

    public: bool
    reason: None | str | Unset = UNSET

    def to_dict(self) -> dict[str, Any]:
        public = self.public

        reason: None | str | Unset
        if isinstance(self.reason, Unset):
            reason = UNSET
        else:
            reason = self.reason

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "public": public,
            }
        )
        if reason is not UNSET:
            field_dict["reason"] = reason

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        public = d.pop("public")

        def _parse_reason(data: object) -> None | str | Unset:
            if data is None:
                return data
            if isinstance(data, Unset):
                return data
            return cast(None | str | Unset, data)

        reason = _parse_reason(d.pop("reason", UNSET))

        set_workspace_public_body = cls(
            public=public,
            reason=reason,
        )

        return set_workspace_public_body
