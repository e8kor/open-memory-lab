from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, Self, TypeVar, cast

from attrs import define as _attrs_define

from ..models.operator_recover_response_202_mail import OperatorRecoverResponse202Mail

if TYPE_CHECKING:
    from ..models.operator_recover_response_202_recovery import (
        OperatorRecoverResponse202Recovery,
    )


T = TypeVar("T", bound="OperatorRecoverResponse202")


@_attrs_define
class OperatorRecoverResponse202:
    """
    Attributes:
        recovery (OperatorRecoverResponse202Recovery):
        notified (list[str]):
        mail (OperatorRecoverResponse202Mail):
        mail_failure (None | str):
    """

    recovery: OperatorRecoverResponse202Recovery
    notified: list[str]
    mail: OperatorRecoverResponse202Mail
    mail_failure: None | str

    def to_dict(self) -> dict[str, Any]:
        recovery = self.recovery.to_dict()

        notified = self.notified

        mail = self.mail.value

        mail_failure: None | str
        mail_failure = self.mail_failure

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "recovery": recovery,
                "notified": notified,
                "mail": mail,
                "mail_failure": mail_failure,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.operator_recover_response_202_recovery import (
            OperatorRecoverResponse202Recovery,
        )

        d = dict(src_dict)
        recovery = OperatorRecoverResponse202Recovery.from_dict(d.pop("recovery"))

        notified = cast(list[str], d.pop("notified"))

        mail = OperatorRecoverResponse202Mail(d.pop("mail"))

        def _parse_mail_failure(data: object) -> None | str:
            if data is None:
                return data
            return cast(None | str, data)

        mail_failure = _parse_mail_failure(d.pop("mail_failure"))

        operator_recover_response_202 = cls(
            recovery=recovery,
            notified=notified,
            mail=mail,
            mail_failure=mail_failure,
        )

        return operator_recover_response_202
