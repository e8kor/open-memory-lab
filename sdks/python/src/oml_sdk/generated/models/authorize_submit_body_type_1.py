from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar

from attrs import define as _attrs_define

from ..models.authorize_submit_body_type_1_action import AuthorizeSubmitBodyType1Action

T = TypeVar("T", bound="AuthorizeSubmitBodyType1")


@_attrs_define
class AuthorizeSubmitBodyType1:
    """Step 3: the confirmation, only after a verified code.

    Attributes:
        handle (str): The authorization request's handle the page carries.
        action (AuthorizeSubmitBodyType1Action):
    """

    handle: str
    action: AuthorizeSubmitBodyType1Action

    def to_dict(self) -> dict[str, Any]:
        handle = self.handle

        action = self.action.value

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "handle": handle,
                "action": action,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        handle = d.pop("handle")

        action = AuthorizeSubmitBodyType1Action(d.pop("action"))

        authorize_submit_body_type_1 = cls(
            handle=handle,
            action=action,
        )

        return authorize_submit_body_type_1
