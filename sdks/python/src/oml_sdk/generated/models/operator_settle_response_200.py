from __future__ import annotations

from collections.abc import Mapping
from typing import TYPE_CHECKING, Any, Self, TypeVar, cast

from attrs import define as _attrs_define

from ..models.operator_settle_response_200_mail import OperatorSettleResponse200Mail

if TYPE_CHECKING:
    from ..models.admin_act import AdminAct


T = TypeVar("T", bound="OperatorSettleResponse200")


@_attrs_define
class OperatorSettleResponse200:
    """
    Attributes:
        act (AdminAct): One admin act (ADR-0097 d2): the confirm screen of d23 reads target (the key's role, principal,
            used_by, the root's person as user, and principal_names_person where the principal spells a person's name) and
            grants. Since v0.8.6.
        notified (list[str]):
        mail (OperatorSettleResponse200Mail):
        mail_failure (None | str):
    """

    act: AdminAct
    notified: list[str]
    mail: OperatorSettleResponse200Mail
    mail_failure: None | str

    def to_dict(self) -> dict[str, Any]:
        act = self.act.to_dict()

        notified = self.notified

        mail = self.mail.value

        mail_failure: None | str
        mail_failure = self.mail_failure

        field_dict: dict[str, Any] = {}

        field_dict.update(
            {
                "act": act,
                "notified": notified,
                "mail": mail,
                "mail_failure": mail_failure,
            }
        )

        return field_dict

    @classmethod
    def from_dict(cls, src_dict: Mapping[str, Any]) -> Self:
        from ..models.admin_act import AdminAct

        d = dict(src_dict)
        act = AdminAct.from_dict(d.pop("act"))

        notified = cast(list[str], d.pop("notified"))

        mail = OperatorSettleResponse200Mail(d.pop("mail"))

        def _parse_mail_failure(data: object) -> None | str:
            if data is None:
                return data
            return cast(None | str, data)

        mail_failure = _parse_mail_failure(d.pop("mail_failure"))

        operator_settle_response_200 = cls(
            act=act,
            notified=notified,
            mail=mail,
            mail_failure=mail_failure,
        )

        return operator_settle_response_200
