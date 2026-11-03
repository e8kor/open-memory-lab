from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, Self, TypeVar

from attrs import define as _attrs_define
from attrs import field as _attrs_field

from ..models.error_code import ErrorCode
from ..types import UNSET, Unset

if TYPE_CHECKING:
    from ..models.error_envelope_error_details import ErrorEnvelopeErrorDetails


T = TypeVar("T", bound="ErrorEnvelopeError")


@_attrs_define
class ErrorEnvelopeError:
    """
    Attributes:
        code (ErrorCode): The closed error vocabulary; identical to docs/api/errors.md by construction.
        message (str): Human-readable; unstable, never parsed.
        rule (str): The design rule the refusal names; stable per code.
        retryable (bool): True only where the identical request can succeed later (Retry-After header present).
        details (ErrorEnvelopeErrorDetails | Unset):
    """

    code: ErrorCode
    message: str
    rule: str
    retryable: bool
    details: ErrorEnvelopeErrorDetails | Unset = UNSET
    additional_properties: dict[str, Any] = _attrs_field(init=False, factory=dict)

    def to_dict(self) -> dict[str, Any]:
        code = self.code.value

        message = self.message

        rule = self.rule

        retryable = self.retryable

        details: dict[str, Any] | Unset = UNSET
        if not isinstance(self.details, Unset):
            details = self.details.to_dict()

        field_dict: dict[str, Any] = {}
        field_dict.update(self.additional_properties)
        field_dict.update(
            {
                "code": code,
                "message": message,
                "rule": rule,
                "retryable": retryable,
            }
        )
        if details is not UNSET:
            field_dict["details"] = details

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.error_envelope_error_details import ErrorEnvelopeErrorDetails

        d = dict(src_dict)
        code = ErrorCode(d.pop("code"))

        message = d.pop("message")

        rule = d.pop("rule")

        retryable = d.pop("retryable")

        _details = d.pop("details", UNSET)
        details: ErrorEnvelopeErrorDetails | Unset
        if isinstance(_details, Unset):
            details = UNSET
        else:
            details = ErrorEnvelopeErrorDetails.from_dict(_details)

        error_envelope_error = cls(
            code=code,
            message=message,
            rule=rule,
            retryable=retryable,
            details=details,
        )

        error_envelope_error.additional_properties = d
        return error_envelope_error

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
