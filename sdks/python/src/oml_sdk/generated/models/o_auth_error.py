from __future__ import annotations

from collections.abc import Mapping
from typing import Any, Self, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.o_auth_error_error import OAuthErrorError
from ..types import UNSET, Unset

T = TypeVar("T", bound="OAuthError")


@_attrs_define
class OAuthError:
    """RFC 6749 §5.2 / RFC 7591 §3.2.2 error body. error is the RFC code a client branches on; error_description begins
    with the OML code (ErrorCode) and its rule (docs/api/errors.md, the oauth family).

        Attributes:
            error (OAuthErrorError):
            error_description (str | Unset):
    """

    error: OAuthErrorError
    error_description: str | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        error = self.error.value

        error_description = self.error_description

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "error": error,
            }
        )
        if error_description is not UNSET:
            field_dict["error_description"] = error_description

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        d = dict(src_dict)
        error = OAuthErrorError(d.pop("error"))

        error_description = d.pop("error_description", UNSET)

        o_auth_error = cls(
            error=error,
            error_description=error_description,
        )

        o_auth_error.additional_properties = d
        return o_auth_error

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
